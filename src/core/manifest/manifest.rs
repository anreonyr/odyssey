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
/// becomes ~7 lines of fluent builder calls. Defaults fill in
/// the convention:
///
/// | Field          | Default               |
/// |----------------|-----------------------|
/// | `version`      | `"0.1.0"`             |
/// | `in_type`      | `"any"`               |
/// | `out_type`     | `"any"`               |
/// | `streaming`    | `false`               |
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
/// Phase 9: the previous `.streaming(bool)` setter is gone —
/// `CapabilityDecl.streaming` is now `CapabilityDecl.kind`
/// (`CapKind`). Builders declare the kind directly via
/// `.kind(CapKind::Stream)` for streaming caps; the default
/// stays `CapKind::Sync`.
pub struct ManifestBuilder {
    name: String,
    version: String,
    cap_name: String,
    cap_contract: String,
    cap_in_type: String,
    cap_out_type: String,
    cap_kind: CapKind,
    requires: Vec<CapabilityRequirement>,
    host: Vec<HostServiceRef>,
    timeout_ms: Option<u32>,
}

impl ManifestBuilder {
    /// Start a manifest for a plugin named `plugin_name`,
    /// exposing one capability whose name and contract are
    /// `cap_name` and `cap_contract`.
    pub fn new(
        plugin_name: impl Into<String>,
        cap_name: impl Into<String>,
        cap_contract: impl Into<String>,
    ) -> Self {
        Self {
            name: plugin_name.into(),
            version: "0.1.0".into(),
            cap_name: cap_name.into(),
            cap_contract: cap_contract.into(),
            cap_in_type: "any".into(),
            cap_out_type: "any".into(),
            cap_kind: CapKind::Sync,
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

    /// Override the capability's input type (default `"any"`).
    pub fn in_type(mut self, t: impl Into<String>) -> Self {
        self.cap_in_type = t.into();
        self
    }

    /// Override the capability's output type (default `"any"`).
    pub fn out_type(mut self, t: impl Into<String>) -> Self {
        self.cap_out_type = t.into();
        self
    }

    /// Set the capability's runtime kind (default `CapKind::Sync`).
    pub fn kind(mut self, k: CapKind) -> Self {
        self.cap_kind = k;
        self
    }

    /// Append a `[[requires]]` entry. May be called multiple
    /// times to declare multiple capability-keyed dependencies.
    pub fn requires(
        mut self,
        handle: impl Into<String>,
        contract: impl Into<String>,
    ) -> Self {
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
            cap_name,
            cap_contract,
            cap_in_type,
            cap_out_type,
            cap_kind,
            requires,
            host,
            timeout_ms,
        } = self;
        PluginManifest {
            plugin: PluginId { name, version },
            isolate: IsolationMode::InProc,
            exposes: vec![CapabilityDecl {
                name: cap_name,
                in_type: cap_in_type,
                out_type: cap_out_type,
                kind: cap_kind,
                contract_name: cap_contract,
            }],
            requires,
            host,
            resources: ResourceHints { timeout_ms },
        }
    }
}
