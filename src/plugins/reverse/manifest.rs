//! Manifest for the reverse plugin — Phase 3 single-source-of-truth.

use std::sync::OnceLock;

use crate::host::manifest::PluginManifest;
use crate::host::manifest::ManifestBuilder;

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
