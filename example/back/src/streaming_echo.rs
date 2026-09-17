//! Streaming echo builtin — pass-through that emits `count`
//! chunks then a terminal `Done`.
//!
//! Phase 11: the first end-to-end `Resource::open` demo.
//! The streaming infrastructure (`Resource::open` returning
//! `mpsc::Receiver<CapabilityChunk>`, `Capability::open` typed
//! path, the `/api/stream` SSE bridge) has been in place since
//! Phase 8 — this builtin exercises it for the first time.
//!
//! Input shape: `{ "text": "<string>", "count": N }`. The handler
//! emits `N` `Item` chunks of `{ "text": <input>, "index": i }`
//! for `i ∈ [0, N)`, then one `CapabilityChunk::Done` chunk.
//!
//! Optional `delay_ms` field adds a `tokio::time::sleep` between
//! chunks; default 0 (no delay). The delay exists so a manual
//! frontend demo can SEE the streaming effect — over localhost
//! the chunks otherwise arrive in a single TCP packet and
//! look like a synchronous dump. Set `delay_ms: 150` for
//! visibly progressive SSE events in the browser.
//!
//! The producer task is `tokio::spawn`-ed from inside
//! `Resource::open`. It owns `tx: mpsc::Sender<CapabilityChunk>`
//! and a copy of `text` + `count`. Cancellation comes from the
//! standard mpsc `tx.send().await` returning `Err(SendError(_))`
//! when the consumer drops the receiver — the same pattern
//! Phase 5 commit `acab1df` documented for the
//! `stream-cancel-during-revoke-test`. No `tokio::select!`,
//! no external cancellation signal — the receiver drop *is*
//! the cancellation signal.

use std::sync::Arc;

use tokio::sync::mpsc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::core::meta::chunk::CapabilityChunk;
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};

/// Streaming echo resource — `open` emits `count` chunks then
/// `Done` over an mpsc channel.
pub struct StreamingEchoResource;

impl Resource for StreamingEchoResource {
    fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                format!(
                    "streaming_echo: expected {{\"text\": \"<string>\"}}, got {}",
                    input
                )
            })?
            .to_string();
        let count = input.get("count").and_then(|v| v.as_u64()).ok_or_else(|| {
            format!(
                "streaming_echo: expected {{\"count\": <u64>}}, got {}",
                input
            )
        })?;
        let delay_ms = input.get("delay_ms").and_then(|v| v.as_u64()).unwrap_or(0);

        let (tx, rx) = mpsc::channel::<CapabilityChunk>(16);
        tokio::spawn(async move {
            for i in 0..count {
                if delay_ms > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }
                let chunk = CapabilityChunk::Item(json!({
                    "text": text,
                    "index": i,
                }));
                if tx.send(chunk).await.is_err() {
                    // Receiver dropped — consumer revoke, SSE
                    // client disconnect, or cspace revoke_tree.
                    // Producer exits gracefully; the cspace's
                    // `revoke_tree` on teardown will close the
                    // receiver and trip this branch.
                    return;
                }
            }
            // Best-effort: send Done. If the consumer is already
            // gone the channel is closed and we exit.
            let _ = tx.send(CapabilityChunk::Done).await;
        });
        Ok(rx)
    }
}

/// Concrete streaming echo builtin. Exposes `manifest()` (via
/// the `BuiltinManifest` trait) and `mint()` (typed concrete
/// method that calls the generic factory with
/// `Arc<StreamingEchoResource>`).
pub struct StreamingEchoBuiltin;

impl BuiltinManifest for StreamingEchoBuiltin {
    fn manifest(&self) -> PluginManifest {
        // The output is a stream — JSON Schema doesn't have a
        // first-class "stream of T" primitive, so we describe
        // each chunk's shape and note the count. The agent
        // advertises the tool as a streaming one; the LLM
        // sees the kind and the schema in concert.
        let tool_schema = serde_json::json!({
            "description": "Echoes `text` back `count` times as a stream of `count + 1` chunks (the last is a terminal Done). Optional `delay_ms` between chunks; default 0.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The text to echo on every chunk."
                    },
                    "count": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Number of payload chunks to emit before Done."
                    },
                    "delay_ms": {
                        "type": "integer",
                        "minimum": 0,
                        "description": "Sleep between chunks; default 0. Set to ~150 for visible streaming over SSE."
                    }
                },
                "required": ["text", "count"]
            },
            "output_schema": {
                "description": "Stream of N+1 chunks: N payload chunks then one Done sentinel.",
                "type": "object",
                "properties": {
                    "text": { "type": "string" },
                    "index": { "type": "integer", "description": "0-based chunk index in [0, count)." }
                }
            }
        });
        ManifestBuilder::new("streaming_echo")
            .expose_streaming_with_schema("streaming_echo", "streaming_echo", tool_schema)
            .timeout_ms(5000)
            .build()
    }
}

impl StreamingEchoBuiltin {
    /// Typed mint — Slice 3 demonstration: streaming_echo's
    /// only cap lives in the plugin's own `PluginCspace`. We
    /// then grant a derived slot into the orchestrator's
    /// global cspace so the HTTP bridge can still look it up
    /// by name. The returned `SlotId` is the GLOBAL one — the
    /// orchestrator's existing teardown path
    /// (`default_ruin` revoking the returned ids) works
    /// unchanged.
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
    ) -> SlotId {
        use odyssey::core::rights::rights::{CapabilityRights, OperationRights};

        let pc = factory.plugin_cspace(plugin);
        let local_slot = pc.mint(kind, decl, budget.clone(), Arc::new(StreamingEchoResource));
        let rights = CapabilityRights {
            operations: OperationRights::ALL,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<StreamingEchoResource>(local_slot, factory.space(), rights, decl.name.clone())
            .expect("grant from plugin cspace to global should succeed")
    }

    /// Phase 11: colocated registration helper. Returns the
    /// `(manifest, mint_fn, ruin_fn)` triple. `streaming_echo`
    /// uses `default_ruin` today (no drain hook) — the slot is
    /// reserved for the future when streaming drain is added.
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            StreamingEchoBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                StreamingEchoBuiltin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }
}
