//! Agent Resources + the `AgentRuntimeBuiltin` plugin
//! manifest. One Resource per cap; the builtin manifest
//! exposes all 8 caps on a single plugin named `"agent"`.
//!
//! Each Resource holds an `Arc<AgentRuntime>`; the 8 mints
//! share the runtime across all caps (so a session table
//! lookup from `agent_resume` finds the same session an
//! `agent_cancel` later drops).

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::core::meta::chunk::CapabilityChunk;
use odyssey::core::rights::rights::CapabilityRights;
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::{CapabilityFactory, MintError, TypedBindings};
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};

use crate::agent::runtime::AgentRuntime;
use crate::agent::types::{
    NAME_CANCEL, NAME_LOAD, NAME_PLAN, NAME_RECALL, NAME_RECORD, NAME_RESUME, NAME_START,
    NAME_STREAM, Observation, REACHES,
};
use tokio::sync::broadcast;

// `step_to_value` re-export — the function lives in `types`
// but callers expect it via `agent::builtin::step_to_value`.
pub use crate::agent::types::step_to_value;

// ---------------------------------------------------------------------------
// Resources — one per cap
// ---------------------------------------------------------------------------

pub struct AgentStartResource {
    pub runtime: Arc<AgentRuntime>,
}

impl Resource for AgentStartResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let goal = input
            .get("goal")
            .and_then(Value::as_str)
            .ok_or_else(|| "agent_start: expected `goal` string".to_string())?
            .to_string();
        let context = input.get("context").cloned().unwrap_or(Value::Null);
        let allowed_tools: Vec<String> = input
            .get("allowed_tools")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        let limits = parse_limits(input.get("limits"));
        let id = self
            .runtime
            .start(goal, context, allowed_tools, limits)
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "session_id": id.to_string(),
            "profile": { "session_id": id.to_string() }
        }))
    }
}

pub struct AgentResumeResource {
    pub runtime: Arc<AgentRuntime>,
}

impl Resource for AgentResumeResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let id = input
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "agent_resume: expected `session_id` string".to_string())?;
        let observation = parse_observation(input.get("observation"))?;
        let (step, history, status) = self
            .runtime
            .advance(id, observation)
            .map_err(|e| e.to_string())?;
        Ok(json!({
            "step": step_to_value(&step),
            "history": history,
            "status": status.as_str(),
        }))
    }
}

pub struct AgentCancelResource {
    pub runtime: Arc<AgentRuntime>,
}

impl Resource for AgentCancelResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let id = input
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "agent_cancel: expected `session_id` string".to_string())?;
        let path = input
            .get("path")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty());
        let history = self.runtime.cancel(id, path).map_err(|e| e.to_string())?;
        let mut out = json!({ "status": "Cancelled", "history": history });
        if let Some(p) = path {
            out["checkpoint_path"] = json!(p);
        }
        Ok(out)
    }
}

pub struct AgentPlanResource {
    pub runtime: Arc<AgentRuntime>,
}

impl Resource for AgentPlanResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let goal = input
            .get("goal")
            .and_then(Value::as_str)
            .ok_or_else(|| "agent_plan: expected `goal` string".to_string())?;
        let tools: Vec<String> = input
            .get("tools")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        self.runtime.plan(goal, &tools).map_err(|e| e.to_string())
    }
}

pub struct AgentStreamResource {
    pub runtime: Arc<AgentRuntime>,
}

impl Resource for AgentStreamResource {
    fn open(&self, input: Value) -> Result<tokio::sync::mpsc::Receiver<CapabilityChunk>, String> {
        let id = input
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "agent_stream: expected `session_id` string".to_string())?;
        self.runtime.lookup(id).map_err(|e| e.to_string())?;

        let mut rx_bcast = {
            let sessions = self.runtime.sessions.lock().expect("sessions poisoned");
            let session = sessions
                .get(&crate::agent::types::SessionId(id.to_string()))
                .ok_or_else(|| format!("agent_stream: session `{id}` not found"))?;
            session.sender.subscribe()
        };

        let (tx, rx) = tokio::sync::mpsc::channel::<CapabilityChunk>(32);

        let id_owned = id.to_string();
        tokio::spawn(async move {
            loop {
                match rx_bcast.recv().await {
                    Ok(ev) => {
                        let chunk = CapabilityChunk::Item(ev.to_value());
                        if tx.send(chunk).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        let chunk = CapabilityChunk::Item(json!({
                            "kind": "lagged",
                            "missed": n,
                            "session_id": id_owned,
                        }));
                        if tx.send(chunk).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        let chunk = CapabilityChunk::Done;
                        let _ = tx.send(chunk).await;
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

pub struct AgentMemoryRecallResource {
    pub runtime: Arc<AgentRuntime>,
}

impl Resource for AgentMemoryRecallResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let query = input
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| "agent_memory_recall: expected `query` string".to_string())?;
        let top_k = input
            .get("top_k")
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .unwrap_or(5);
        let filter_tags: Vec<String> = input
            .get("filter")
            .and_then(|f| f.get("tags"))
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        self.runtime
            .recall(query, top_k, filter_tags)
            .map_err(|e| e.to_string())
    }
}

pub struct AgentMemoryRecordResource {
    pub runtime: Arc<AgentRuntime>,
}

impl Resource for AgentMemoryRecordResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let content = input
            .get("content")
            .cloned()
            .ok_or_else(|| "agent_memory_record: expected `content`".to_string())?;
        let tags: Vec<String> = input
            .get("tags")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        self.runtime
            .record(content, tags)
            .map_err(|e| e.to_string())
    }
}

pub struct AgentLoadResource {
    pub runtime: Arc<AgentRuntime>,
}

impl Resource for AgentLoadResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let path = input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "agent_load: expected `path` string".to_string())?;
        let sid = self.runtime.load(path).map_err(|e| e.to_string())?;
        Ok(json!({ "session_id": sid }))
    }
}

// ---------------------------------------------------------------------------
// Input parsing helpers
// ---------------------------------------------------------------------------

fn parse_limits(v: Option<&Value>) -> crate::agent::types::SessionLimits {
    let mut l = crate::agent::types::SessionLimits::default();
    if let Some(obj) = v.and_then(Value::as_object) {
        if let Some(n) = obj.get("max_steps").and_then(Value::as_u64) {
            l.max_steps = n as u32;
        }
        if let Some(n) = obj.get("max_idle_ms").and_then(Value::as_u64) {
            l.max_idle_ms = n as u32;
        }
        if let Some(n) = obj.get("max_session_ms").and_then(Value::as_u64) {
            l.max_session_ms = n as u32;
        }
    }
    l
}

fn parse_observation(v: Option<&Value>) -> Result<Observation, String> {
    let v = v.ok_or_else(|| "agent_resume: expected `observation`".to_string())?;
    let kind = v
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| "agent_resume: observation.kind required".to_string())?;
    match kind {
        "Tick" => Ok(Observation::Tick),
        "UserReply" => {
            let text = v
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| "UserReply.text required".to_string())?
                .to_string();
            Ok(Observation::UserReply(text))
        }
        "ToolResult" => {
            let tool = v
                .get("tool")
                .and_then(Value::as_str)
                .ok_or_else(|| "ToolResult.tool required".to_string())?
                .to_string();
            let value = v.get("value").cloned().unwrap_or(Value::Null);
            let error = v
                .get("error")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            Ok(Observation::ToolResult { tool, value, error })
        }
        other => Err(format!("agent_resume: unknown observation.kind `{other}`")),
    }
}

// ---------------------------------------------------------------------------
// Builtin — one plugin, eight caps
// ---------------------------------------------------------------------------

pub struct AgentRuntimeBuiltin;

impl BuiltinManifest for AgentRuntimeBuiltin {
    type Resource = AgentStartResource;
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("agent")
            .expose(NAME_START, crate::agent::types::CONTRACT_START)
            .expose(NAME_RESUME, crate::agent::types::CONTRACT_RESUME)
            .expose(NAME_CANCEL, crate::agent::types::CONTRACT_CANCEL)
            .expose(NAME_PLAN, crate::agent::types::CONTRACT_PLAN)
            .expose_streaming(NAME_STREAM, crate::agent::types::CONTRACT_STREAM)
            .expose(NAME_RECALL, crate::agent::types::CONTRACT_RECALL)
            .expose(NAME_RECORD, crate::agent::types::CONTRACT_RECORD)
            .expose(NAME_LOAD, crate::agent::types::CONTRACT_LOAD)
            .requires(REACHES[0].0, REACHES[0].1)
            .requires(REACHES[1].0, REACHES[1].1)
            .timeout_ms(30000)
            .build()
    }
}

impl AgentRuntimeBuiltin {
    /// Typed mint — Slice 3 pattern: each cap mints into the
    /// plugin's own `PluginCspace`, then `grant_to` exports
    /// to the orchestrator's global cspace.
    pub fn mint(
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        bindings: &[ResolvedBinding],
        typed_bindings: &TypedBindings,
    ) -> Result<SlotId, MintError> {
        let runtime = Arc::new(AgentRuntime::new(
            factory.space().clone(),
            typed_bindings.clone(),
            bindings.to_vec(),
        ));
        match decl.name.as_str() {
            NAME_START => mint_cap(
                factory,
                plugin,
                kind,
                decl,
                budget,
                Arc::new(AgentStartResource {
                    runtime: runtime.clone(),
                }),
            ),
            NAME_RESUME => mint_cap(
                factory,
                plugin,
                kind,
                decl,
                budget,
                Arc::new(AgentResumeResource {
                    runtime: runtime.clone(),
                }),
            ),
            NAME_CANCEL => mint_cap(
                factory,
                plugin,
                kind,
                decl,
                budget,
                Arc::new(AgentCancelResource {
                    runtime: runtime.clone(),
                }),
            ),
            NAME_PLAN => mint_cap(
                factory,
                plugin,
                kind,
                decl,
                budget,
                Arc::new(AgentPlanResource {
                    runtime: runtime.clone(),
                }),
            ),
            NAME_STREAM => mint_cap(
                factory,
                plugin,
                kind,
                decl,
                budget,
                Arc::new(AgentStreamResource {
                    runtime: runtime.clone(),
                }),
            ),
            NAME_RECALL => mint_cap(
                factory,
                plugin,
                kind,
                decl,
                budget,
                Arc::new(AgentMemoryRecallResource {
                    runtime: runtime.clone(),
                }),
            ),
            NAME_RECORD => mint_cap(
                factory,
                plugin,
                kind,
                decl,
                budget,
                Arc::new(AgentMemoryRecordResource {
                    runtime: runtime.clone(),
                }),
            ),
            NAME_LOAD => mint_cap(
                factory,
                plugin,
                kind,
                decl,
                budget,
                Arc::new(AgentLoadResource {
                    runtime: runtime.clone(),
                }),
            ),
            other => panic!("agent: unexpected capability name `{other}`"),
        }
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            AgentRuntimeBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                AgentRuntimeBuiltin::mint(
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

/// Slice 3 helper: mint a single resource into the plugin's
/// own cspace, then grant a derived slot into the
/// orchestrator's global cspace.
fn mint_cap<R>(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    kind: CapKind,
    decl: &CapabilityDecl,
    budget: CapabilityBudget,
    resource: Arc<R>,
) -> Result<SlotId, MintError>
where
    R: Resource + 'static,
{
    use odyssey::core::rights::rights::Rights;

    let pc = factory.plugin_cspace(plugin);
    let local_slot = pc.mint(kind, decl, budget.clone(), resource);
    let rights = CapabilityRights {
        // agent is the only builtin that holds REVOKE:
        // session lifecycle (cancel) revokes the session-bus
        // cap so the model's invoke_dyn no longer routes
        // deltas to a dead session.
        operations: Rights::INVOKE | Rights::ASSIGN | Rights::REVOKE,
        timeout_ms: budget.timeout_ms(),
    };
    pc.inner()
        .grant_to::<R>(local_slot, factory.space(), rights, decl.name.clone())
        .map_err(|e| MintError::GrantFailed {
            plugin: plugin.name.clone(),
            cap: decl.name.clone(),
            source: e,
        })
}
