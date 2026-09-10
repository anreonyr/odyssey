//! Plugin manifest — the data contract between plugin author and host kernel.
//!
//! TOML describes identity, the exposed capability surface
//! (including the JSON-Schema contract the `RuleAgent` and HTTP
//! bridge consult), dependencies on other plugins' capabilities,
//! host services the plugin needs, and resource hints.
//!
//! Today every plugin compiles into the host binary as an in-proc
//! module (`InProc` isolation). Designs for WASM / cdylib /
//! subprocess loaders — including the manifest shapes those
//! loaders must honour — live under `docs/deferred/`.

use serde::{Deserialize, Serialize};

pub use crate::kernel::ids::PluginId;
use crate::kernel::meta::{AuthorityContract, Protocol};
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
    ///
    /// This is the **preferred** way to declare dependencies going
    /// forward. The legacy `consumes` field (plugin-version-keyed)
    /// is kept for backward compatibility but new plugins should
    /// use `requires`.
    #[serde(default)]
    pub requires: Vec<CapabilityRequirement>,
    /// Legacy plugin-version-keyed dependency declaration.
    /// Deprecated as of Phase 3 P3.1; use `requires` instead.
    /// Kept so existing manifests continue to parse until the
    /// next major version removes the field.
    #[serde(default)]
    pub consumes: Vec<DependencyRef>,
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
pub struct DependencyRef {
    pub plugin: String,
    pub version: String,
    pub capability: String,
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