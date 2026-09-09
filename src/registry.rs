//! Plugin registry — accepts manifests, rejects duplicates.
//!
//! This is the data-only half of what was previously a richer service: it
//! records which manifests have been loaded and rejects double-registration.
//! Query helpers (`find`, `list`) were removed when the dependency check
//! moved into `main.rs`'s Phase 5 (which compares against minted tokens
//! directly).

use std::collections::HashMap;
use std::sync::Mutex;

use crate::manifest::{PluginId, PluginManifest};

#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    #[error("plugin already loaded: {0}@{1}", .id.name, .id.version)]
    AlreadyLoaded { id: PluginId },
}

#[derive(Default)]
pub struct Registry {
    by_name: Mutex<HashMap<String, Vec<PluginManifest>>>,
}

impl Registry {
    pub fn register(&self, m: PluginManifest) -> Result<(), RegistryError> {
        let mut by_name = self.by_name.lock().expect("registry mutex poisoned");
        let versions = by_name.entry(m.plugin.name.clone()).or_default();
        if versions.iter().any(|v| v.plugin == m.plugin) {
            return Err(RegistryError::AlreadyLoaded { id: m.plugin.clone() });
        }
        // Sort newest-first so future find() can take the first hit.
        versions.push(m);
        versions.sort_by(|a, b| {
            let va = semver::Version::parse(&a.plugin.version).ok();
            let vb = semver::Version::parse(&b.plugin.version).ok();
            vb.partial_cmp(&va).unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(())
    }
}