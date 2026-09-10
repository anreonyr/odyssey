//! Phase 4 P4.1 — HTTP plugin manifest.
//!
//! Exposes one capability, `http_request`, with one action:
//!
//! | action    | operation bit    | operation         |
//! | --------- | ---------------- | ----------------- |
//! | `request` | `HTTP_REQUEST`   | POST / GET / ...  |
//!
//! Sync capability. Real HTTP backends (reqwest/ureq) will
//! still go through `Resource::invoke` — only the handler's
//! response lookup changes.

use std::sync::OnceLock;

use crate::host::manifest::PluginManifest;
use crate::host::manifest::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("http", "http_request", "http_request")
            .in_type("http_request")
            .out_type("http_response")
            .action("request", "HTTP_REQUEST")
            .timeout_ms(5000)
            .build()
    })
}
