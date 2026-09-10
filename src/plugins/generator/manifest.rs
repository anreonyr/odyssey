//! Plugin manifest — Phase 3 single-source-of-truth, with
//! Phase 4 P4.1 `[[requires]]` for HTTP-backed generation.

use std::sync::OnceLock;

use crate::kernel::manifest::PluginManifest;
use crate::kernel::manifest_builder::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("generator", "generate", "generate")
            .in_type("prompt")
            .out_type("tokens")
            .streaming(true)
            .action("generate", "GENERATE")
            // Phase 4 P4.1 — generator now holds a capability
            // dependency on `http` (Capability<HTTP>). The
            // resolver reads this and computes the binding; the
            // HttpModel looks up the http capability at
            // generation time via the binding table.
            .requires("http", "http_request")
            .timeout_ms(30000)
            .build()
    })
}
