//! Database builtin — in-memory key-value store.
//!
//! Phase 5 plugin (was `plugins/database/`). Demoted to a builtin
//! in Phase 8.
//!
//! Operation is selected by the JSON-RPC `op` field:
//! - `"get"` — read `key`. Returns `{ "ok": true, "value": <v> }` if
//!   present, `{ "ok": false }` if missing.
//! - `"set"` — write `key` to `value`. Returns `{ "ok": true }`.
//! - `"delete"` — remove `key`. Returns `{ "ok": true }`.
//!
//! State lives in a `RwLock<HashMap<String, Value>>` inside the
//! resource. Multiple `Arc<DatabaseResource>` clones share the
//! same backing store (the `Arc<RwLock<...>>`).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use odyssey::capability::resource::Resource;
use odyssey::core::identity::ids::PluginId;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use serde_json::{json, Value};

type Store = Arc<RwLock<HashMap<String, Value>>>;

pub struct DatabaseResource {
    store: Store,
}

impl Resource for DatabaseResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let op = input
            .get("op")
            .and_then(|v| v.as_str())
            .ok_or_else(|| format!("database: expected {{\"op\": \"<get|set|delete>\", ...}}, got {}", input))?;

        match op {
            "get" => {
                let key = input.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    format!("database.get: expected {{\"op\":\"get\",\"key\":\"<string>\"}}, got {}", input)
                })?;
                let store = self.store.read().map_err(|e| format!("database: lock poisoned: {e}"))?;
                Ok(match store.get(key) {
                    Some(value) => json!({ "ok": true, "value": value }),
                    None => json!({ "ok": false }),
                })
            }
            "set" => {
                let key = input.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    format!("database.set: expected {{\"op\":\"set\",\"key\":\"<string>\",\"value\":...}}, got {}", input)
                })?;
                let value = input.get("value").cloned().ok_or_else(|| {
                    format!("database.set: expected {{\"op\":\"set\",\"key\":\"<string>\",\"value\":...}}, got {}", input)
                })?;
                let mut store = self.store.write().map_err(|e| format!("database: lock poisoned: {e}"))?;
                store.insert(key.to_string(), value);
                Ok(json!({ "ok": true }))
            }
            "delete" => {
                let key = input.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    format!("database.delete: expected {{\"op\":\"delete\",\"key\":\"<string>\"}}, got {}", input)
                })?;
                let mut store = self.store.write().map_err(|e| format!("database: lock poisoned: {e}"))?;
                store.remove(key);
                Ok(json!({ "ok": true }))
            }
            other => Err(format!("database: unknown op {other:?}; expected get|set|delete")),
        }
    }
}

/// Manifest for the database plugin.
pub fn manifest() -> PluginManifest {
    ManifestBuilder::new("database", "database", "database")
        .host("dispatcher")
        .action("get", "READ")
        .action("set", "WRITE")
        .action("delete", "WRITE")
        .timeout_ms(5000)
        .build()
}

/// Construct the typed database handler. Returns an
/// `Arc<DatabaseResource>` whose backing store is shared with
/// other clones.
pub fn mint(_plugin: &PluginId, _decl: &CapabilityDecl) -> Arc<DatabaseResource> {
    Arc::new(DatabaseResource {
        store: Arc::new(RwLock::new(HashMap::new())),
    })
}
