//! Plugin manifest — data shape + builder + TOML loader + validate.
//!
//! Phase 8: this file is the merge of the Phase 5 split
//! (`types.rs` + `builder.rs` + `load.rs` + `mod.rs`) into one
//! cohesive module. The split was justified while personality
//! owned the manifest types; in the new layout the manifest is
//! a leaf shared value type, and one file is enough.
//!
//! Manifests are TOML files (or fluent `ManifestBuilder` chains
//! in plugin code) describing plugin identity, the exposed
//! capability surface (including the JSON-Schema contract the
//! `RuleAgent` and HTTP bridge consult), dependencies on other
//! plugins' capabilities, host services the plugin needs, and
//! resource hints.
//!
//! Today every plugin compiles into the host binary as an in-proc
//! module (`InProc` isolation). Designs for WASM / cdylib /
//! subprocess loaders — including the manifest shapes those
//! loaders must honour — live under `docs/deferred/`.

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use crate::core::identity::ids::PluginId;
use crate::core::meta::meta::{AuthorityContract, Protocol};

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

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CapabilityDecl {
    pub name: String,
    /// Logical type name for input (declared in manifest). Surfaced to the
    /// HTTP bridge for clients; not yet enforced as a runtime type check.
    #[serde(default)]
    #[allow(dead_code)]
    pub in_type: String,
    /// Logical type name for output (declared in manifest). Surfaced to the
    /// HTTP bridge for clients; not yet enforced as a runtime type check.
    #[serde(default)]
    #[allow(dead_code)]
    pub out_type: String,
    #[serde(default)]
    pub streaming: bool,
    /// Phase 3 P3.1 — the contract name this capability publishes.
    /// Other plugins declare a matching `contract` in their
    /// `[[requires]]` block to be bound to this capability at boot.
    /// Empty string means "no contract published" — such caps are
    /// only reachable by direct slot lookup, not by capability
    /// injection. Conventional form is lowercase dotted, e.g.
    /// `"generator"`, `"embedder"`, `"database"`, `"agent"`.
    #[serde(default)]
    pub contract_name: String,
    /// Phase 3 P3.2 — Authority vocabulary (action →
    /// `OperationRights` map). The runtime path's only
    /// authority input; `RuleAgent` reads
    /// `authority.operation_for(action)` to translate verbs
    /// to bits. Optional — empty authority means "no
    /// enumerable action surface".
    #[serde(default)]
    pub authority: AuthorityContract,
    /// Phase 3 P3.2 — Wire-protocol metadata (schemas,
    /// description, transport, version, media_type). Pure
    /// metadata; the runtime never validates against it.
    /// Optional — empty protocol means "no advertised wire
    /// metadata".
    #[serde(default)]
    pub protocol: Protocol,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HostServiceRef {
    pub service: String,
    #[serde(default)]
    pub scope: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ResourceHints {
    pub cpu: Option<String>,
    pub mem_mb: Option<u32>,
    pub io_bps: Option<u64>,
    pub timeout_ms: Option<u32>,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("parse: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("validation: {0}")]
    Validation(String),
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.plugin.name.is_empty() {
            return Err(ManifestError::Validation("plugin.name is empty".into()));
        }
        if self.plugin.version.is_empty() {
            return Err(ManifestError::Validation("plugin.version is empty".into()));
        }
        if self.exposes.is_empty() {
            return Err(ManifestError::Validation(
                "plugin must expose at least one capability".into(),
            ));
        }
        for cap in &self.exposes {
            if cap.name.is_empty() {
                return Err(ManifestError::Validation(
                    "capability.name is empty".into(),
                ));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// TOML loader
// ---------------------------------------------------------------------------

/// Parse a TOML string into a [`PluginManifest`].
pub fn from_toml_str(s: &str) -> Result<PluginManifest, ManifestError> {
    let m: PluginManifest = toml::from_str(s)?;
    m.validate()?;
    Ok(m)
}

/// Load a manifest from a TOML file on disk.
pub fn from_path(p: impl AsRef<Path>) -> Result<PluginManifest, ManifestError> {
    let text = std::fs::read_to_string(p)?;
    from_toml_str(&text)
}

impl PluginManifest {
    /// Convenience re-export so existing call sites
    /// (`PluginManifest::from_toml_str`, `from_path`) keep
    /// compiling after the loader moves into its own module.
    pub fn from_toml_str(s: &str) -> Result<Self, ManifestError> {
        from_toml_str(s)
    }

    pub fn from_path(p: impl AsRef<Path>) -> Result<Self, ManifestError> {
        from_path(p)
    }
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
pub struct ManifestBuilder {
    name: String,
    version: String,
    cap_name: String,
    cap_contract: String,
    cap_in_type: String,
    cap_out_type: String,
    cap_streaming: bool,
    requires: Vec<CapabilityRequirement>,
    host: Vec<HostServiceRef>,
    timeout_ms: Option<u32>,
    actions: Vec<(String, String)>,
    protocol: Protocol,
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
            cap_streaming: false,
            requires: Vec::new(),
            host: Vec::new(),
            timeout_ms: None,
            actions: Vec::new(),
            protocol: Protocol::empty(),
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

    /// Mark the capability as streaming (default `false`).
    pub fn streaming(mut self, s: bool) -> Self {
        self.cap_streaming = s;
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

    /// Append a `[[exposes.authority.actions]]` entry — publishes
    /// the action → `OperationRights` mapping that
    /// type-agnostic dispatchers (RuleAgent) consult. Call once
    /// per action. The authority defaults to `empty()` if never
    /// called.
    ///
    /// Empty name or operation are rejected with `assert!`:
    /// an empty action name is unreachable from any dispatch
    /// (silent dead row), and an empty operation string is
    /// unknown to `OperationRights::parse` (dispatch fails with
    /// a confusing 'not a known operation' error). Fail loud at
    /// manifest-build time instead.
    pub fn action(
        mut self,
        name: impl Into<String>,
        operation: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let operation = operation.into();
        assert!(
            !name.is_empty(),
            "ManifestBuilder::action: action name cannot be empty"
        );
        assert!(
            !operation.is_empty(),
            "ManifestBuilder::action: operation for action {name:?} cannot be empty"
        );
        self.actions.push((name, operation));
        self
    }

    /// Set the wire-protocol metadata in one call. Phase 3
    /// P3.2 — the manifest's `protocol` block is pure
    /// metadata (schemas, description, transport, version,
    /// media_type); it doesn't affect dispatch.
    pub fn protocol(mut self, p: Protocol) -> Self {
        self.protocol = p;
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
            cap_streaming,
            requires,
            host,
            timeout_ms,
            actions,
            protocol,
        } = self;
        let mut authority = AuthorityContract::empty();
        for (a, op) in actions {
            authority = authority.with_action(a, op);
        }
        PluginManifest {
            plugin: PluginId { name, version },
            isolate: IsolationMode::InProc,
            exposes: vec![CapabilityDecl {
                name: cap_name,
                in_type: cap_in_type,
                out_type: cap_out_type,
                streaming: cap_streaming,
                contract_name: cap_contract,
                authority,
                protocol,
            }],
            requires,
            host,
            resources: ResourceHints {
                timeout_ms,
                ..Default::default()
            },
        }
    }
}
