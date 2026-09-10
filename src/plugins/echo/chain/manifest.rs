//! Manifest for the echo_chain plugin — Phase 3 single-source-of-truth.
//!
//! Note the `.consumes(...)` line: it preserves the legacy
//! `[[consumes]]` block that the boot validation log prints.
//! The resolver walks `.requires(...)`; the consumes is
//! informational only.

use std::sync::OnceLock;

use crate::host::manifest::PluginManifest;
use crate::host::manifest::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("echo-chain", "echo_chain", "echo_chain")
            .requires("echo", "echo")
            .consumes("echo", "^0.1", "echo")
            .host("dispatcher")
            .host("registry")
            .timeout_ms(200)
            .build()
    })
}
