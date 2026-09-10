//! Manifest for the slow plugin — Phase 3 single-source-of-truth.

use std::sync::OnceLock;

use crate::kernel::manifest::PluginManifest;
use crate::kernel::manifest_builder::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("slow", "slow", "slow")
            .host("dispatcher")
            .timeout_ms(50)
            .build()
    })
}
