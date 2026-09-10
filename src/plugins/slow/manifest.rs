//! Manifest for the slow plugin — Phase 3 single-source-of-truth.

use std::sync::OnceLock;

use crate::host::manifest::PluginManifest;
use crate::host::manifest::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("slow", "slow", "slow")
            .host("dispatcher")
            .timeout_ms(50)
            .build()
    })
}
