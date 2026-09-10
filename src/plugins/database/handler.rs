//! Database plugin — `DatabaseResource: Resource`. Sync only.
//!
//! Phase 4 P4.3 — a primitive "dumb" capability. The resource
//! exposes three operations (READ / WRITE / ADMIN) under one
//! capability slot (`database`). Authority enforcement lives
//! in the consumer: before dispatching an op, the consumer
//! checks `meta.authority.operation_for(op)` and refuses if the
//! granted authority does not include that action. This keeps
//! the resource small and makes the P4.5 "same program,
//! different env" proof tractable — the agent's program reads
//! its reachable capabilities and decides at runtime.
//!
//! Storage is an in-memory `RwLock<HashMap<String, String>>`
//! keyed by string. Phase 4 is about *composition*, not
//! persistence. A real backend (sqlite, sled, ...) drops in
//! behind the same `invoke()` shape later.
//!
//! ## Why std::sync::RwLock, not tokio::sync::RwLock
//!
//! `Resource::invoke` is sync and is called from inside the
//! HTTP bridge's tokio runtime. `tokio::sync::RwLock`'s
//! `blocking_write` panics when called from an async context.
//! Critical sections here are short (HashMap ops), so blocking
//! the OS thread is acceptable for Phase 4. A real backend
//! would replace this with a connection pool behind an async
//! interface.
//!
//! ## Input shape
//!
//! Every invocation is a JSON object with an `op` field
//! selecting the operation:
//!
//! ```json
//! { "op": "read",        "key": "k" }
//! { "op": "write",       "key": "k", "value": "v" }
//! { "op": "admin_list" }
//! ```
//!
//! Returns are JSON:
//!
//! - `read`        → `{"value": <string|null>}` (missing key → null)
//! - `write`       → `{"ok": true}`
//! - `admin_list`  → `{"entries": [{"key": "k", "value": "v"}, ...]}`

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use cordis::{plugin_with, Context, Injection, LogLevel, Plugin};
use serde_json::{json, Value};

use crate::capability::{Resource, Slot};

type Store = Arc<RwLock<HashMap<String, String>>>;

pub struct DatabaseResource {
    store: Store,
}

impl DatabaseResource {
    pub fn new() -> Self {
        Self { store: Arc::new(RwLock::new(HashMap::new())) }
    }

    pub fn with_handle(handle: Store) -> Self {
        Self { store: handle }
    }

    /// Shared handle to the underlying store. Useful for tests
    /// that want to seed state before invoking, or for
    /// post-mortem inspection.
    pub fn store(&self) -> Store {
        Arc::clone(&self.store)
    }
}

impl Default for DatabaseResource {
    fn default() -> Self {
        Self::new()
    }
}

impl Resource for DatabaseResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let op = input
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| "database: missing 'op' field".to_string())?;
        match op {
            "read" => {
                let key = input
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "database.read: missing 'key'".to_string())?;
                let store = self
                    .store
                    .read()
                    .map_err(|e| format!("database.read: lock poisoned: {e}"))?;
                Ok(json!({ "value": store.get(key).cloned() }))
            }
            "write" => {
                let key = input
                    .get("key")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "database.write: missing 'key'".to_string())?;
                let value = input
                    .get("value")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "database.write: missing 'value'".to_string())?;
                let mut store = self
                    .store
                    .write()
                    .map_err(|e| format!("database.write: lock poisoned: {e}"))?;
                store.insert(key.to_string(), value.to_string());
                Ok(json!({ "ok": true }))
            }
            "admin_list" => {
                let store = self
                    .store
                    .read()
                    .map_err(|e| format!("database.admin_list: lock poisoned: {e}"))?;
                let entries: Vec<Value> = store
                    .iter()
                    .map(|(k, v)| json!({ "key": k, "value": v }))
                    .collect();
                Ok(json!({ "entries": entries }))
            }
            other => Err(format!("database: unknown op '{other}'")),
        }
    }
}

/// Build a fresh `DatabaseResource`. Boot uses one of these
/// per plugin instance; tests can share an underlying store by
/// calling [`DatabaseResource::with_handle`] instead.
pub fn handler() -> Arc<DatabaseResource> {
    Arc::new(DatabaseResource::new())
}

pub fn database_plugin() -> Arc<dyn Plugin> {
    plugin_with(
        "database",
        vec![Injection::from("slot:database")],
        |ctx: Context, _cfg: ()| async move {
            let slot: Arc<Slot<DatabaseResource>> = ctx.require("slot:database")?;
            ctx.logger().log(
                LogLevel::Info,
                format!(
                    "database plugin: slot={} cap_id={}",
                    slot.id().raw(),
                    slot.capability()
                        .map(|c| c.id().to_string())
                        .unwrap_or_else(|| "(empty)".to_string()),
                )
                .into(),
            );
            Ok(())
        },
    )
}

// ---------------------------------------------------------------------------
// Unit tests — exercise the resource directly without going through mint.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn invoke(resource: &DatabaseResource, json_body: &str) -> Result<Value, String> {
        resource.invoke(serde_json::from_str(json_body).unwrap())
    }

    #[test]
    fn write_then_read_returns_value() {
        let r = DatabaseResource::new();
        invoke(&r, r#"{"op":"write","key":"k","value":"v"}"#).unwrap();
        let out = invoke(&r, r#"{"op":"read","key":"k"}"#).unwrap();
        assert_eq!(out, json!({"value": "v"}));
    }

    #[test]
    fn read_missing_key_returns_null() {
        let r = DatabaseResource::new();
        let out = invoke(&r, r#"{"op":"read","key":"absent"}"#).unwrap();
        assert_eq!(out, json!({"value": null}));
    }

    #[test]
    fn write_overwrites_existing_value() {
        let r = DatabaseResource::new();
        invoke(&r, r#"{"op":"write","key":"k","value":"v1"}"#).unwrap();
        invoke(&r, r#"{"op":"write","key":"k","value":"v2"}"#).unwrap();
        let out = invoke(&r, r#"{"op":"read","key":"k"}"#).unwrap();
        assert_eq!(out, json!({"value": "v2"}));
    }

    #[test]
    fn admin_list_enumerates_all_keys() {
        let r = DatabaseResource::new();
        invoke(&r, r#"{"op":"write","key":"a","value":"1"}"#).unwrap();
        invoke(&r, r#"{"op":"write","key":"b","value":"2"}"#).unwrap();
        let out = invoke(&r, r#"{"op":"admin_list"}"#).unwrap();
        let entries = out.get("entries").unwrap().as_array().unwrap();
        assert_eq!(entries.len(), 2);
        let keys: std::collections::HashSet<&str> = entries
            .iter()
            .filter_map(|e| e.get("key").and_then(Value::as_str))
            .collect();
        assert!(keys.contains("a"));
        assert!(keys.contains("b"));
    }

    #[test]
    fn missing_op_field_is_error() {
        let r = DatabaseResource::new();
        let err = invoke(&r, r#"{"key":"x"}"#).unwrap_err();
        assert!(err.contains("missing 'op'"));
    }

    #[test]
    fn unknown_op_is_error() {
        let r = DatabaseResource::new();
        let err = invoke(&r, r#"{"op":"drop_table"}"#).unwrap_err();
        assert!(err.contains("unknown op 'drop_table'"));
    }

    #[test]
    fn read_missing_key_field_is_error() {
        let r = DatabaseResource::new();
        let err = invoke(&r, r#"{"op":"read"}"#).unwrap_err();
        assert!(err.contains("database.read: missing 'key'"));
    }

    #[test]
    fn write_missing_value_field_is_error() {
        let r = DatabaseResource::new();
        let err = invoke(&r, r#"{"op":"write","key":"k"}"#).unwrap_err();
        assert!(err.contains("database.write: missing 'value'"));
    }

    #[test]
    fn manifest_publishes_three_authority_actions() {
        let m = crate::plugins::database::manifest::manifest();
        let authority = &m.exposes[0].authority;
        assert!(authority.operation_for("read").is_some());
        assert!(authority.operation_for("write").is_some());
        assert!(authority.operation_for("admin").is_some());
        assert_eq!(authority.operation_for("read").unwrap(), "DB_READ");
        assert_eq!(authority.operation_for("write").unwrap(), "DB_WRITE");
        assert_eq!(authority.operation_for("admin").unwrap(), "DB_ADMIN");
    }
}
