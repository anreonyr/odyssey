//! Manifest for the generator plugin — Phase 3 single-source-of-truth.

use std::sync::OnceLock;

use crate::kernel::manifest::PluginManifest;
use crate::kernel::manifest_builder::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("generator", "generate", "generator")
            .in_type("text")
            .out_type("token")
            .streaming(true)
            .host("dispatcher")
            .timeout_ms(30000)
            .build()
    })
}
