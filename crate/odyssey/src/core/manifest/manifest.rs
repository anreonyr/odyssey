//! Plugin manifest — data shape + builder.
//!
//! Phase 8: this file is the merge of the Phase 5 split
//! (`types.rs` + `builder.rs` + `load.rs` + `mod.rs`) into one
//! cohesive module. The split was justified while personality
//! owned the manifest types; in the new layout the manifest is
//! a leaf shared value type, and one file is enough.
//!
//! Phase 9 cleanup: the TOML loader (`from_toml_str`,
//! `from_path`) and the `validate` method are deleted. The
//! Phase 5/6/7 era of plugins loading manifests from disk is
//! over; every runtime plugin (and now every builtin) builds
//! its manifest in code via `ManifestBuilder`. The `toml` and
//! `serde::Deserialize` derives stay so a future TOML file
//! can re-attach if the deferred WASM / cdylib loaders come
//! back, but the wiring helpers are gone.
//!
//! Manifests describe plugin identity, the exposed capability
//! surface, and dependencies on other plugins' capabilities.

use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::core::identity::ids::PluginId;
use crate::core::identity::kind::CapKind;
use crate::core::manifest::bundle::BundleId;

// ---------------------------------------------------------------------------
// Data shapes
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginId,
    /// Optional bundle identity. When `Some`, the plugin belongs
    /// to a named bundle — used by `ResolvedPlan::render` to group
    /// the mint order in boot diagrams, and by `ResolveError`
    /// variants' `requester_bundle` field to attribute failures to
    /// a bundle. The kernel does NOT use this for dispatch
    /// (`plugin_registry` keys by `plugin.name`, unchanged);
    /// `None` is the default for plugins that ship outside any
    /// bundle context.
    ///
    /// `skip_serializing_if = "Option::is_none"` keeps the wire
    /// format stable for pre-bundle manifests — a JSON file that
    /// omits the field round-trips to `None`, and a manifest with
    /// `None` serializes without the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<BundleId>,
    #[serde(default)]
    pub exposes: Vec<CapabilityDecl>,
    /// Phase 3 P3.1 — capability-keyed dependencies. The resolver
    /// matches each `requires[*].contract` against another
    /// manifest's `[[exposes]] contract_name`. A plugin declares
    /// what contract it needs, not which plugin provides it; the
    /// resolver decides. Empty `requires` means the plugin is a
    /// root provider (or has no capability dependencies).
    #[serde(default)]
    pub requires: Vec<CapabilityRequirement>,
    /// Per-call wall-clock budget in milliseconds. Default `None`
    /// lets the orchestrator's `CapabilityBudget::new(5000)` use
    /// the host default.
    #[serde(default)]
    pub timeout_ms: Option<u32>,
}

/// Phase 3 P3.1 — a capability-keyed dependency.
///
/// A plugin's runtime behaviour is determined by which contracts
/// it can bind to. The resolver at boot time scans every loaded
/// manifest, builds a `contract_name → provider plugin` index,
/// then walks each plugin's `requires` list to determine mint
/// order and bindings.
///
/// Example:
/// ```toml
/// [[requires]]
/// name     = "embedder"      # handle inside this plugin
/// contract = "embedder"      # the contract to bind against
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityRequirement {
    /// Local handle name. The plugin code uses this string to
    /// look up the received slot reference. By convention,
    /// matches the provider's capability name but isn't required
    /// to — the handle is purely local.
    pub name: String,
    /// Contract name to bind against. Must match exactly one
    /// `[[exposes]] contract_name` from another loaded manifest,
    /// or boot fails with `ResolveError::Unprovided` (no
    /// provider). With multiple providers, see `priority`.
    pub contract: String,
    /// DI Phase 21: priority hint for multi-provider selection.
    /// `None` (default) is equivalent to `Some(0)` for
    /// comparison purposes. Higher `priority` wins; equal
    /// priority falls back to `(version, name)` lex tie-break.
    /// When the resolver's `priority`-filter still leaves
    /// multiple top-priority providers, boot succeeds with a
    /// warning rather than a hard fail (`ResolveError::Ambiguous`
    /// is reserved for the empty-providers case).
    #[serde(default)]
    pub priority: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityDecl {
    pub name: String,
    /// Runtime kind (sync vs stream). Phase 9 cleanup: replaced
    /// the previous `streaming: bool` field. The bool could only
    /// express two states; the `CapKind` enum is the single
    /// source of truth for kind across the kernel, the runtime
    /// meta, and the manifest. Serde's `default` keeps the
    /// manifest TOML shape honest (a missing `kind` parses as
    /// `Sync`); the legacy `streaming: bool` field is no longer
    /// recognised.
    #[serde(default)]
    pub kind: CapKind,
    /// Phase 3 P3.1 — the contract name this capability publishes.
    /// Other plugins declare a matching `contract` in their
    /// `[[requires]]` block to be bound to this capability at boot.
    /// Empty string means "no contract published" — such caps are
    /// only reachable by direct slot lookup, not by capability
    /// injection. Conventional form is lowercase dotted, e.g.
    /// `"generator"`, `"embedder"`, `"database"`, `"agent"`.
    #[serde(default)]
    pub contract_name: String,
    /// Optional tool schema — a JSON object the agent reads via the
    /// `tool_descriptor` capability to learn the tool's input/output
    /// contract. `None` means "no schema published" — the cap is not
    /// agent-callable in a schema-driven way (an agent can still call
    /// it by name through the cspace, just without structured schema
    /// metadata). The value is opaque to the kernel; it's the
    /// `tool_descriptor` plugin that interprets it.
    #[serde(default)]
    pub tool_schema: Option<Value>,
    /// DI Phase 21: priority hint used by the resolver when
    /// multiple providers publish the same contract. Highest
    /// `priority` wins; equal priority falls back to
    /// `(version, name)` lex tie-break, with a boot-time
    /// warning instead of a hard fail (`AmbiguousPriority`).
    /// `None` (default) is equivalent to `Some(0)` for
    /// comparison purposes — the resolver treats the
    /// absence of a hint as "no preference, pick me last".
    /// `Some(0)` is rejected at `validate()` time
    /// (`EmptyPriority`) for symmetry with the consumer-side
    /// `requires_with_priority`: if you mean "no hint", drop
    /// the field.
    #[serde(default)]
    pub priority: Option<u32>,
}

// ---------------------------------------------------------------------------
// Fluent builder
// ---------------------------------------------------------------------------

/// Builder for [`PluginManifest`] — Phase 3 manifest template.
///
/// Replaces the toml parser + the 50-line struct-literal each
/// plugin used to write. Each runtime plugin's `manifest.rs`
/// becomes a few lines of fluent builder calls. Defaults fill in
/// the convention:
///
/// | Field          | Default               |
/// |----------------|-----------------------|
/// | `version`      | `"0.1.0"`             |
/// | `kind`         | `CapKind::Sync`       |
/// | `exposes`      | `[]`                  |
/// | `requires`     | `[]`                  |
/// | `timeout_ms`   | `None` → host default |
///
/// Phase 9: the previous `.action(name, op)` and `.protocol(p)`
/// setters are gone — they fed the now-deleted
/// `AuthorityContract` / `Protocol` fields that were
/// write-only metadata. If a future cap needs to advertise a
/// typed contract vocabulary, add it back at the same time the
/// reader is added.
pub struct ManifestBuilder {
    name: String,
    version: String,
    bundle: Option<BundleId>,
    exposes: Vec<CapabilityDecl>,
    requires: Vec<CapabilityRequirement>,
    timeout_ms: Option<u32>,
}

impl ManifestBuilder {
    /// Start a manifest for a plugin named `plugin_name`. The
    /// plugin's capabilities are appended with [`Self::expose`] /
    /// [`Self::expose_streaming`]; a manifest with none is a legal
    /// but useless plugin, so `build` does not reject it.
    pub fn new(plugin_name: impl Into<String>) -> Self {
        Self {
            name: plugin_name.into(),
            version: "0.1.0".into(),
            bundle: None,
            exposes: Vec::new(),
            requires: Vec::new(),
            timeout_ms: None,
        }
    }

    /// Override the plugin version (default `"0.1.0"`).
    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = v.into();
        self
    }

    /// Tag this manifest as a member of bundle
    /// `(name, version)`. Used by `Bundle::register()` wrappers
    /// at the example layer to stamp every member manifest with
    /// the same bundle id; the kernel then groups the mint order
    /// in boot diagrams and attributes `ResolveError` failures
    /// to the bundle that contained the offending plugin.
    ///
    /// Additive and chainable, matching every other setter on
    /// `ManifestBuilder` (precedent `202deae` — the singular
    /// `.in_type` setter went with the deleted `in_type` field;
    /// `bundle` is a non-replacing chainable setter).
    ///
    /// `Some(BundleId)` is set unconditionally; calling `.bundle()`
    /// a second time replaces the prior value (the only setter on
    /// `ManifestBuilder` that does this). The intent is that a
    /// bundle author calls `.bundle()` exactly once per manifest,
    /// in concert with the rest of the bundle's members.
    pub fn bundle(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        self.bundle = Some(BundleId::new(name, version));
        self
    }

    /// Append a sync capability. `contract_name` is what another
    /// plugin's `requires` matches against, so it is required
    /// here even though `CapabilityDecl::contract_name` allows an
    /// empty string for caps that publish nothing.
    pub fn expose(mut self, name: impl Into<String>, contract_name: impl Into<String>) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            kind: CapKind::Sync,
            contract_name: contract_name.into(),
            tool_schema: None,
            priority: None,
        });
        self
    }

    /// Append a sync capability and attach a `tool_schema` JSON
    /// object. Use this when the capability is meant to be called by
    /// the agent through `tool_descriptor`; the schema is opaque to
    /// the kernel.
    pub fn expose_with_schema(
        mut self,
        name: impl Into<String>,
        contract_name: impl Into<String>,
        tool_schema: Value,
    ) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            kind: CapKind::Sync,
            contract_name: contract_name.into(),
            tool_schema: Some(tool_schema),
            priority: None,
        });
        self
    }

    /// Append a sync capability with a priority hint. Used by
    /// DI's multi-provider resolution: when two plugins publish
    /// the same `contract_name`, the resolver picks the
    /// highest-`priority` provider. Tie → warning + lex-min.
    /// `priority=0` is rejected at `validate()` time; use the
    /// no-priority [`Self::expose`] for "default preference".
    pub fn expose_with_priority(
        mut self,
        name: impl Into<String>,
        contract_name: impl Into<String>,
        priority: u32,
    ) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            kind: CapKind::Sync,
            contract_name: contract_name.into(),
            tool_schema: None,
            priority: Some(priority),
        });
        self
    }

    /// Append a streaming capability. Separate from [`Self::expose`]
    /// because the kind is the whole difference and `CapKind` has
    /// exactly two variants: a `kind` parameter would let a caller
    /// pass `Sync` to a method named `expose` and read as though
    /// they had asked for a stream.
    pub fn expose_streaming(
        mut self,
        name: impl Into<String>,
        contract_name: impl Into<String>,
    ) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            kind: CapKind::Stream,
            contract_name: contract_name.into(),
            tool_schema: None,
            priority: None,
        });
        self
    }

    /// Append a streaming capability with a `tool_schema`.
    /// Symmetric with [`Self::expose_with_schema`]; the
    /// schema's `output_schema` describes one chunk of the
    /// stream, and the cap's `kind` tells the LLM the result
    /// is a stream of those.
    pub fn expose_streaming_with_schema(
        mut self,
        name: impl Into<String>,
        contract_name: impl Into<String>,
        tool_schema: Value,
    ) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            kind: CapKind::Stream,
            contract_name: contract_name.into(),
            tool_schema: Some(tool_schema),
            priority: None,
        });
        self
    }

    /// Append a `[[requires]]` entry. May be called multiple
    /// times to declare multiple capability-keyed dependencies.
    pub fn requires(mut self, handle: impl Into<String>, contract: impl Into<String>) -> Self {
        self.requires.push(CapabilityRequirement {
            name: handle.into(),
            contract: contract.into(),
            priority: None,
        });
        self
    }

    /// Append a `[[requires]]` entry with a priority hint.
    /// Used by DI's multi-provider resolution: highest
    /// `priority` wins; equal priority falls back to lex-min
    /// on `(version, name)`. `Some(0)` is rejected at
    /// `validate()` time (`EmptyPriority`) — drop the hint
    /// entirely if you want to participate as a default.
    pub fn requires_with_priority(
        mut self,
        handle: impl Into<String>,
        contract: impl Into<String>,
        priority: u32,
    ) -> Self {
        self.requires.push(CapabilityRequirement {
            name: handle.into(),
            contract: contract.into(),
            priority: Some(priority),
        });
        self
    }

    /// Set the timeout hint in milliseconds (default `None`,
    /// which lets `CapabilityBudget::new` use the host default).
    pub fn timeout_ms(mut self, ms: u32) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// Materialise the manifest. Consumes the builder.
    pub fn build(self) -> PluginManifest {
        let ManifestBuilder {
            name,
            version,
            bundle,
            exposes,
            requires,
            timeout_ms,
        } = self;
        PluginManifest {
            plugin: PluginId { name, version },
            bundle,
            exposes,
            requires,
            timeout_ms,
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime loader (Slice 5 of Direction A)
// ---------------------------------------------------------------------------

/// Errors produced by the runtime manifest loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestLoadError {
    /// JSON parse failure (caller passed invalid JSON).
    Parse(String),
    /// Field-level validation failure (parse succeeded but
    /// the manifest doesn't satisfy the loader's invariants).
    Invalid(String),
    /// I/O failure (file read, missing path, etc.).
    Io(String),
}

impl std::fmt::Display for ManifestLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "manifest parse: {msg}"),
            Self::Invalid(msg) => write!(f, "manifest invalid: {msg}"),
            Self::Io(msg) => write!(f, "manifest io: {msg}"),
        }
    }
}

impl std::error::Error for ManifestLoadError {}

/// Outcome of `validate` — what the manifest is missing,
/// suitable for surfacing in error messages or operator UIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestInvalid {
    EmptyPluginName,
    EmptyPluginVersion,
    NoExposes,
    EmptyExposeName {
        index: usize,
    },
    EmptyExposeContract {
        index: usize,
    },
    DuplicateExposeName {
        name: String,
    },
    EmptyRequireHandle {
        index: usize,
    },
    EmptyRequireContract {
        index: usize,
    },
    /// DI Phase 21: `priority: Some(0)` is suspicious — it
    /// suggests the author meant "unimportant" and should drop
    /// the hint entirely (let it default to `None`). Catching
    /// it at validate time avoids silent priority-zero ties at
    /// boot.
    EmptyPriority {
        index: usize,
    },
}

impl std::fmt::Display for ManifestInvalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPluginName => write!(f, "plugin.name is empty"),
            Self::EmptyPluginVersion => write!(f, "plugin.version is empty"),
            Self::NoExposes => write!(f, "plugin exposes no capabilities"),
            Self::EmptyExposeName { index } => {
                write!(f, "exposes[{index}].name is empty")
            }
            Self::EmptyExposeContract { index } => {
                write!(f, "exposes[{index}].contract_name is empty")
            }
            Self::DuplicateExposeName { name } => {
                write!(f, "exposes contains duplicate name `{name}`")
            }
            Self::EmptyRequireHandle { index } => {
                write!(f, "requires[{index}].name is empty")
            }
            Self::EmptyRequireContract { index } => {
                write!(f, "requires[{index}].contract is empty")
            }
            Self::EmptyPriority { index } => {
                write!(
                    f,
                    "exposes[{index}].priority or requires[{index}].priority is 0; \
                     use the no-priority setter (expose / requires) instead of \
                     expose_with_priority / requires_with_priority with 0"
                )
            }
        }
    }
}

impl PluginManifest {
    /// Parse a manifest from a JSON string. Performs the
    /// same field validation as the builder path — a
    /// manifest that round-trips through this function
    /// would build to the same value via `ManifestBuilder`.
    ///
    /// Slice 5: the JSON format is the canonical
    /// serialization of `PluginManifest`. The previous
    /// TOML loader (`from_toml_str`) was removed in Phase 9;
    /// a future WASM / cdylib loader could re-attach TOML
    /// by adding the `toml` dep and calling `serde::Deserialize`
    /// on the same struct shape — the schema is decoupled
    /// from the on-disk format.
    pub fn from_json_str(s: &str) -> Result<Self, ManifestLoadError> {
        let m: PluginManifest =
            serde_json::from_str(s).map_err(|e| ManifestLoadError::Parse(e.to_string()))?;
        m.validate()
            .map_err(|e| ManifestLoadError::Invalid(e.to_string()))?;
        Ok(m)
    }

    /// Load a manifest from a file path. Reads as UTF-8 and
    /// delegates to `from_json_str`. The file extension is
    /// not consulted — the parser is JSON either way. A
    /// future change can dispatch on extension to support
    /// multiple formats.
    pub fn from_path(path: &Path) -> Result<Self, ManifestLoadError> {
        let bytes = std::fs::read(path).map_err(|e| ManifestLoadError::Io(e.to_string()))?;
        let s = std::str::from_utf8(&bytes)
            .map_err(|e| ManifestLoadError::Io(format!("not utf-8: {e}")))?;
        Self::from_json_str(s)
    }

    /// Validate the manifest's invariants. Returns the
    /// first violation as a `ManifestInvalid`; subsequent
    /// violations are not enumerated (the loader fails
    /// fast on the first problem).
    pub fn validate(&self) -> Result<(), ManifestInvalid> {
        if self.plugin.name.is_empty() {
            return Err(ManifestInvalid::EmptyPluginName);
        }
        if self.plugin.version.is_empty() {
            return Err(ManifestInvalid::EmptyPluginVersion);
        }
        if self.exposes.is_empty() {
            return Err(ManifestInvalid::NoExposes);
        }
        // Track seen expose names for duplicate detection.
        // A duplicate would silently shadow itself at lookup
        // time (the second install replaces the first in
        // the cspace's `names` map).
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for (i, e) in self.exposes.iter().enumerate() {
            if e.name.is_empty() {
                return Err(ManifestInvalid::EmptyExposeName { index: i });
            }
            if e.contract_name.is_empty() {
                return Err(ManifestInvalid::EmptyExposeContract { index: i });
            }
            if !seen.insert(e.name.as_str()) {
                return Err(ManifestInvalid::DuplicateExposeName {
                    name: e.name.clone(),
                });
            }
            // DI Phase 21: catch suspicious zero priority at
            // validate time. Use `expose` (not
            // `expose_with_priority`) to express "no hint".
            // Same rule as the consumer-side `requires_with_priority`;
            // both flavours share the `EmptyPriority` variant since
            // the fix is the same: drop the field.
            if matches!(e.priority, Some(0)) {
                return Err(ManifestInvalid::EmptyPriority { index: i });
            }
        }
        for (i, r) in self.requires.iter().enumerate() {
            if r.name.is_empty() {
                return Err(ManifestInvalid::EmptyRequireHandle { index: i });
            }
            if r.contract.is_empty() {
                return Err(ManifestInvalid::EmptyRequireContract { index: i });
            }
            // DI Phase 21: catch suspicious zero priority at
            // validate time. Use `requires` (not
            // `requires_with_priority`) to express "no hint".
            if matches!(r.priority, Some(0)) {
                return Err(ManifestInvalid::EmptyPriority { index: i });
            }
        }
        Ok(())
    }
}
