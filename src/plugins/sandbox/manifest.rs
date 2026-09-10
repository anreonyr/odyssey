//! Manifest for the sandbox plugin — Phase 3 single-source-of-truth.

use std::sync::OnceLock;

use crate::host::manifest::PluginManifest;
use crate::host::manifest::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("sandbox", "exec", "sandbox")
            .host("dispatcher")
            .build()
    })
}
