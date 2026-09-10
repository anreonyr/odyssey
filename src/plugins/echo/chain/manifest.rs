//! Manifest for the echo_chain plugin — Phase 3 single-source-of-truth.
//!
//! Phase 6: the legacy `.consumes(...)` call was removed; the
//! resolver walks `.requires(...)` exclusively. Chain declares
//! its echo dependency via `requires` so the resolver binds it
//! at boot.

use std::sync::OnceLock;

use crate::host::manifest::PluginManifest;
use crate::host::manifest::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("echo-chain", "echo_chain", "echo_chain")
            .requires("echo", "echo")
            .host("dispatcher")
            .host("registry")
            .timeout_ms(200)
            .build()
    })
}
