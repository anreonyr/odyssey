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

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use crate::core::identity::ids::PluginId;
use crate::core::identity::kind::CapKind;

// ---------------------------------------------------------------------------
// Data shapes
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginId,
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
    /// provider) or `ResolveError::Ambiguous` (multiple providers
    /// without a priority hint).
    pub contract: String,
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
        });
        self
    }

    /// Append a `[[requires]]` entry. May be called multiple
    /// times to declare multiple capability-keyed dependencies.
    pub fn requires(mut self, handle: impl Into<String>, contract: impl Into<String>) -> Self {
        self.requires.push(CapabilityRequirement {
            name: handle.into(),
            contract: contract.into(),
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
            exposes,
            requires,
            timeout_ms,
        } = self;
        PluginManifest {
            plugin: PluginId { name, version },
            exposes,
            requires,
            timeout_ms,
        }
    }
}
