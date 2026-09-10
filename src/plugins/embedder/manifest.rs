//! Phase 4 P4.2 — Embedder plugin manifest.
//!
//! Exposes one capability, `embed`, with one action in its
//! authority contract:
//!
//! | action   | operation bit | operation                |
//! | -------- | ------------- | ------------------------ |
//! | `embed`  | `EMBED`       | `embed(text) → Vec<f32>` |
//!
//! Sync capability (`is_streaming = false`). The output type
//! is `vector` (a fixed-dimension float array).

use std::sync::OnceLock;

use crate::host::manifest::PluginManifest;
use crate::host::manifest::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("embedder", "embed", "embed")
            .in_type("text")
            .out_type("vector")
            .action("embed", "EMBED")
            .timeout_ms(2000)
            .build()
    })
}
