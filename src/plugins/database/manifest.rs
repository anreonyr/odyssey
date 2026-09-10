//! Phase 4 P4.3 — Database plugin manifest.
//!
//! Exposes one capability, `database`, with three actions in
//! its authority contract:
//!
//! | action    | operation bit    | operation         |
//! | --------- | ---------------- | ----------------- |
//! | `read`    | `DB_READ`        | `read(key) → v`   |
//! | `write`   | `DB_WRITE`       | `write(k,v)`      |
//! | `admin`   | `DB_ADMIN`       | `admin_list()`    |
//!
//! Authority enforcement lives in the *consumer* (P4.4 agent).
//! The resource itself happily dispatches any of the three ops;
//! the consumer checks `meta.authority.operation_for(op)` before
//! invoking and refuses if the granted authority doesn't include
//! that action.

use std::sync::OnceLock;

use crate::host::manifest::PluginManifest;
use crate::host::manifest::ManifestBuilder;

static MANIFEST: OnceLock<PluginManifest> = OnceLock::new();

pub fn manifest() -> &'static PluginManifest {
    MANIFEST.get_or_init(|| {
        ManifestBuilder::new("database", "database", "database")
            .in_type("op_request")
            .out_type("op_response")
            .action("read", "DB_READ")
            .action("write", "DB_WRITE")
            .action("admin", "DB_ADMIN")
            .timeout_ms(2000)
            .build()
    })
}
