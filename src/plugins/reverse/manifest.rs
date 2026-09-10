//! Manifest for the reverse plugin — Phase 3 single-source-of-truth.

use std::sync::OnceLock;

use crate::kernel::manifest::PluginManifest;
use crate::kernel::manifest_builder::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("reverse", "reverse", "reverse")
            .in_type("text")
            .out_type("text")
            .host("dispatcher")
            .build()
    })
}
