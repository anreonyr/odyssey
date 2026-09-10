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
use std::path::Path;

use crate::capability::CapabilityContract;

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct PluginId {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginId,
    /// Always `InProc` today. The variants for Wasm / Subprocess
    /// live in `docs/deferred/`; when those loaders ship they'll
    /// be added back here as additional variants.
    pub isolate: IsolationMode,
    #[serde(default)]
    pub exposes: Vec<CapabilityDecl>,
    #[serde(default)]
    pub consumes: Vec<DependencyRef>,
    #[serde(default)]
    pub host: Vec<HostServiceRef>,
    #[serde(default)]
    pub resources: ResourceHints,
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
    #[allow(dead_code)]
    pub in_type: String,
    /// Logical type name for output (declared in manifest). Surfaced to the
    /// HTTP bridge for clients; not yet enforced as a runtime type check.
    #[allow(dead_code)]
    pub out_type: String,
    pub streaming: bool,
    /// JSON-Schema-style contract for input / output, an action
    /// table that names the RPC verbs the cap accepts and the
    /// `OperationRights` bit each requires, and a one-line
    /// description. Optional — missing field deserialises to an
    /// empty contract so existing manifests keep parsing unchanged.
    /// `CapabilityMeta.contract` carries it from the factory into
    /// the runtime, where the HTTP bridge and any type-aware
    /// caller (the `RuleAgent` in particular) can read it.
    #[serde(default)]
    pub contract: CapabilityContract,
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
    pub fn from_toml_str(s: &str) -> Result<Self, ManifestError> {
        let m: PluginManifest = toml::from_str(s)?;
        m.validate()?;
        Ok(m)
    }

    pub fn from_path(p: impl AsRef<Path>) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(p)?;
        Self::from_toml_str(&text)
    }

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