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
//! surface, dependencies on other plugins' capabilities, host
//! services the plugin needs, and resource hints.
//!
//! Today every plugin compiles into the host binary as an in-proc
//! module (`InProc` isolation). Designs for WASM / cdylib /
//! subprocess loaders — including the manifest shapes those
//! loaders must honour — live under `docs/deferred/`.

use serde::{Deserialize, Serialize};

pub use crate::core::identity::ids::PluginId;
use crate::core::identity::kind::CapKind;

// ---------------------------------------------------------------------------
// Data shapes
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginId,
    /// Always `InProc` today. The variants for Wasm / Subprocess
    /// live in `docs/deferred/`; when those loaders ship they'll
    /// be added back here as additional variants.
    pub isolate: IsolationMode,
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
    #[serde(default)]
    pub host: Vec<HostServiceRef>,
    #[serde(default)]
    pub resources: ResourceHints,
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

/// Isolation mode for a plugin. The runtime currently supports
/// only `InProc` — the plugin's `handler.rs` is compiled into
/// the host binary and the factory mints a typed
/// `Capability<MyResource>` directly. Designs for WASM (cdylib
/// or wasmtime) and subprocess transports are tracked under
/// `docs/deferred/` and will mint into this enum when they ship.
///
/// TOML shape: `[isolate] kind = "in_proc"`. Tagged-enum so adding
/// new variants later is a non-breaking change to existing
/// manifests.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IsolationMode {
    InProc,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityDecl {
    pub name: String,
    /// Logical type name for input (declared in manifest). Surfaced
    /// to the HTTP bridge for clients; not yet enforced as a runtime
    /// type check.
    #[serde(default)]
    pub in_type: String,
    /// Logical type name for output (declared in manifest). Surfaced
    /// to the HTTP bridge for clients; not yet enforced as a runtime
    /// type check.
    #[serde(default)]
    pub out_type: String,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostServiceRef {
    pub service: String,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceHints {
    /// Per-call wall-clock budget in milliseconds. The only field
    /// the kernel actually uses.
    ///
    /// Phase 8 cleanup: `cpu`, `mem_mb`, `io_bps` were serde-
    /// decorated but had zero readers anywhere in `src/`. Same
    /// family as Phase 5 D5 (the `tokens_per_minute` /
    /// `bytes_per_minute` quota fields). Serde's
    /// `deny_unknown_fields = false` silently drops them on load
    /// so no manifest breaks.
    pub timeout_ms: Option<u32>,
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
/// | `in_type`      | `"any"`               |
/// | `out_type`     | `"any"`               |
/// | `kind`         | `CapKind::Sync`       |
/// | `exposes`      | `[]`                  |
/// | `requires`     | `[]`                  |
/// | `host`         | `[]`                  |
/// | `timeout_ms`   | `None` → host default |
///
/// Phase 9: the previous `.action(name, op)` and `.protocol(p)`
/// setters are gone — they fed the now-deleted
/// `AuthorityContract` / `Protocol` fields that were
/// write-only metadata. If a future cap needs to advertise a
/// typed contract vocabulary, add it back at the same time the
/// reader is added.
///
/// The builder used to hold one capability's fields flat
/// (`cap_name`, `cap_contract`, `cap_kind`, ...) and `build`
/// wrapped a single `CapabilityDecl` in a one-element `vec!`.
/// `exposes` is a `Vec` and the agent plugin exposes two
/// capabilities, so the fields move to `expose` /
/// `expose_streaming`, which append to the list. The
/// singular `.in_type` / `.out_type` / `.kind` setters go with
/// them: they could only ever configure one cap, and
/// `CapabilityDecl` still defaults both type names to `"any"`.
pub struct ManifestBuilder {
    name: String,
    version: String,
    exposes: Vec<CapabilityDecl>,
    requires: Vec<CapabilityRequirement>,
    host: Vec<HostServiceRef>,
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
            host: Vec::new(),
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
    ///
    /// `in_type` / `out_type` stay `"any"`: the manifest carries no
    /// type vocabulary beyond that, and every builtin today
    /// declares `"any"`. A cap that needs a real type name gets a
    /// setter at the same time a reader for it exists.
    pub fn expose(mut self, name: impl Into<String>, contract_name: impl Into<String>) -> Self {
        self.exposes.push(CapabilityDecl {
            name: name.into(),
            in_type: "any".into(),
            out_type: "any".into(),
            kind: CapKind::Sync,
            contract_name: contract_name.into(),
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
            in_type: "any".into(),
            out_type: "any".into(),
            kind: CapKind::Stream,
            contract_name: contract_name.into(),
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

    /// Append a `[[host]]` entry. May be called multiple times
    /// to declare multiple host services.
    pub fn host(mut self, service: impl Into<String>) -> Self {
        self.host.push(HostServiceRef {
            service: service.into(),
            scope: None,
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
            host,
            timeout_ms,
        } = self;
        PluginManifest {
            plugin: PluginId { name, version },
            isolate: IsolationMode::InProc,
            exposes,
            requires,
            host,
            resources: ResourceHints { timeout_ms },
        }
    }
}
