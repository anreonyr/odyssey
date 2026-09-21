//! Database builtin — a generic KV-store trait with pluggable
//! backends. The default backend is in-memory; other backends
//! (file-backed, sqlite, ...) can be added under this directory
//! by implementing the `Database` trait and registering a new
//! builtin wrapper.
//!
//! ## Trait shape
//!
//! Three operations, all `&self`:
//! - `get(key)` returns `Some(value)` if present, else `None`.
//! - `set(key, value)` stores; overwrites silently.
//! - `delete(key)` removes the entry.
//!
//! Errors are reported as `String` to match the rest of the
//! kernel's `Resource::invoke` shape.
//!
//! ## Cap
//!
//! One cap per plugin: `database`. The dispatch is on the
//! `op` field (`get` / `set` / `delete`). See the JSON
//! Schema in `DatabaseBuiltin::manifest` for the exact shape.

pub mod in_memory;

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::{CapabilityFactory, MintError, TypedBindings};
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};

pub use in_memory::InMemoryDatabase;

/// Backend-agnostic KV store. Backends live as sibling files
/// under `database/` (e.g. `in_memory.rs`, future
/// `file.rs`, `sqlite.rs`); each is a thin newtype that
/// implements this trait.
pub trait Database: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Value>, String>;
    fn set(&self, key: &str, value: Value) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<bool, String>;
}

/// Resource wrapper that dispatches by `op` over any
/// `Arc<dyn Database>`. The builtin chooses a backend at
/// mint time; the resource itself is backend-agnostic.
pub struct DatabaseResource {
    backend: Arc<dyn Database>,
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
                match self.backend.get(key)? {
                    Some(value) => Ok(json!({ "ok": true, "value": value })),
                    None => Ok(json!({ "ok": false })),
                }
            }
            "set" => {
                let key = input.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    format!("database.set: expected {{\"op\":\"set\",\"key\":\"<string>\",\"value\":...}}, got {}", input)
                })?;
                let value = input.get("value").cloned().ok_or_else(|| {
                    format!("database.set: expected {{\"op\":\"set\",\"key\":\"<string>\",\"value\":...}}, got {}", input)
                })?;
                self.backend.set(key, value)?;
                Ok(json!({ "ok": true }))
            }
            "delete" => {
                let key = input.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    format!("database.delete: expected {{\"op\":\"delete\",\"key\":\"<string>\"}}, got {}", input)
                })?;
                self.backend.delete(key)?;
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
    type Resource = DatabaseResource;
    fn manifest(&self) -> PluginManifest {
        // Discriminated union over `op`. The schema declares
        // `additionalProperties: false` on each variant so
        // the LLM doesn't accidentally pass a `value` on a
        // `get`.
        let tool_schema = serde_json::json!({
            "description": "Backend-agnostic JSON key-value store. Keys are strings, values are any JSON value. The chosen backend determines persistence.",
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
    /// Construct a builtin that wraps the given backend.
    /// Mint routes through the standard PluginCspace + grant_to
    /// pattern; the backend reference lives inside the resource.
    pub fn new() -> Self {
        Self
    }

    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
        _typed_bindings: &TypedBindings,
    ) -> Result<SlotId, MintError> {
        use odyssey::core::rights::rights::{CapabilityRights, Rights};

        let pc = factory.plugin_cspace(plugin);
        let backend: Arc<dyn Database> = Arc::new(InMemoryDatabase::new());
        let local_slot = pc.mint(
            kind,
            decl,
            budget.clone(),
            Arc::new(DatabaseResource { backend }),
        );
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<DatabaseResource>(local_slot, factory.space(), rights, decl.name.clone())
            .map_err(|e| MintError::GrantFailed {
                plugin: plugin.name.clone(),
                cap: decl.name.clone(),
                source: e,
            })
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            DatabaseBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                DatabaseBuiltin.mint(
                    factory,
                    plugin,
                    decl,
                    kind,
                    budget,
                    bindings,
                    typed_bindings,
                )
            },
            default_ruin,
        )
    }
}

impl Default for DatabaseBuiltin {
    fn default() -> Self {
        Self::new()
    }
}
