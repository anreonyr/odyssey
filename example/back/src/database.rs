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

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};

type Store = Arc<RwLock<HashMap<String, Value>>>;

pub struct DatabaseResource {
    store: Store,
}

impl Resource for DatabaseResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let op = input.get("op").and_then(|v| v.as_str()).ok_or_else(|| {
            format!(
                "database: expected {{\"op\": \"<get|set|delete>\", ...}}, got {}",
                input
            )
        })?;

        match op {
            "get" => {
                let key = input.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    format!(
                        "database.get: expected {{\"op\":\"get\",\"key\":\"<string>\"}}, got {}",
                        input
                    )
                })?;
                let store = self
                    .store
                    .read()
                    .map_err(|e| format!("database: lock poisoned: {e}"))?;
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
                let mut store = self
                    .store
                    .write()
                    .map_err(|e| format!("database: lock poisoned: {e}"))?;
                store.insert(key.to_string(), value);
                Ok(json!({ "ok": true }))
            }
            "delete" => {
                let key = input.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    format!("database.delete: expected {{\"op\":\"delete\",\"key\":\"<string>\"}}, got {}", input)
                })?;
                let mut store = self
                    .store
                    .write()
                    .map_err(|e| format!("database: lock poisoned: {e}"))?;
                store.remove(key);
                Ok(json!({ "ok": true }))
            }
            other => Err(format!(
                "database: unknown op {other:?}; expected get|set|delete"
            )),
        }
    }
}

pub struct DatabaseBuiltin;

impl BuiltinManifest for DatabaseBuiltin {
    fn manifest(&self) -> PluginManifest {
        // The database's input shape is discriminated by the
        // `op` field. JSON Schema's `oneOf` expresses the
        // three shapes precisely; the LLM picks the right
        // one based on the op it wants to perform. The
        // in-memory store is per-process — values do not
        // survive a restart. The schema declares the
        // `additionalProperties: false` on each variant so
        // the LLM doesn't accidentally pass a `value` on a
        // `get`.
        let tool_schema = serde_json::json!({
            "description": "In-process JSON key-value store. Keys are strings, values are any JSON value. State is per-process and lost on restart.",
            "input_schema": {
                "oneOf": [
                    {
                        "type": "object",
                        "properties": {
                            "op": { "type": "string", "enum": ["get"] },
                            "key": { "type": "string" }
                        },
                        "required": ["op", "key"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "op": { "type": "string", "enum": ["set"] },
                            "key": { "type": "string" },
                            "value": { "description": "Any JSON value; stored verbatim." }
                        },
                        "required": ["op", "key", "value"],
                        "additionalProperties": false
                    },
                    {
                        "type": "object",
                        "properties": {
                            "op": { "type": "string", "enum": ["delete"] },
                            "key": { "type": "string" }
                        },
                        "required": ["op", "key"],
                        "additionalProperties": false
                    }
                ]
            },
            "output_schema": {
                "oneOf": [
                    {
                        "description": "Returned by `get`.",
                        "type": "object",
                        "properties": {
                            "ok": { "type": "boolean" },
                            "value": { "description": "Present only when `ok` is true." }
                        },
                        "required": ["ok"]
                    },
                    {
                        "description": "Returned by `set` and `delete`.",
                        "type": "object",
                        "properties": {
                            "ok": { "type": "boolean" }
                        },
                        "required": ["ok"]
                    }
                ]
            }
        });
        ManifestBuilder::new("database")
            .expose_with_schema("database", "database", tool_schema)
            .timeout_ms(5000)
            .build()
    }
}

impl DatabaseBuiltin {
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
    ) -> SlotId {
        factory.mint(
            kind,
            decl,
            plugin,
            budget,
            Arc::new(DatabaseResource {
                store: Arc::new(RwLock::new(HashMap::new())),
            }),
        )
    }

    /// Phase 11: colocated registration helper. Returns the
    /// `(manifest, mint_fn, ruin_fn)` triple; see
    /// `builtins/src/echo.rs::register` for rationale.
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            DatabaseBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                DatabaseBuiltin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }
}
