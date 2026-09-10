//! Manifest for the echo_stream plugin — Phase 3 single-source-of-truth.

use std::sync::OnceLock;

use crate::kernel::manifest::PluginManifest;
use crate::kernel::manifest_builder::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("echo_stream", "echo_stream", "echo_stream")
            .in_type("text")
            .out_type("token")
            .streaming(true)
            .host("dispatcher")
            .timeout_ms(30000)
            .build()
    })
}
