//! `AgentRuntime` — the shared core that the 8 agent caps hold.
//!
//! After the refactor: holds an internal `Arc<dyn MemoryBackend>`
//! (no separate `memory` plugin lookup), and reaches the
//! generator / embedder through `AgentSlots` typed by
//! `TypedBindings`. The 2 former memory caps (`memory_query`/
//! `memory_insert`) are gone; their functionality moved into
//! the agent's `agent_memory_recall` / `agent_memory_record`
//! caps which now call `self.memory` directly.

use std::sync::Arc;

use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::core::identity::ids::SlotId;
use odyssey::core::rights::rights::Rights;
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::TypedBindings;
use serde_json::{Value, json};

use crate::agent::memory::{MemoryBackend, insert_record, pick_backend, query_records};
use crate::agent::prompt::{
    build_plan_prompt, build_prompt, build_system_prompt, collect_tool_schemas,
    first_native_tool_call, parse_llm_reply,
};
use crate::agent::session::global_sessions;
use crate::agent::types::{
    AgentError, AgentEvent, AgentSlots, Observation, Session, SessionCheckpoint, SessionId,
    SessionLimits, SessionStatus, Step, ToolInvocation, ToolResult,
};

pub struct AgentRuntime {
    pub cspace: CapabilitySpace,
    /// DI Phase 21: typed bindings for the 2 external
    /// providers (generator, embedder).
    pub typed_bindings: TypedBindings,
    /// Kept for the read-only introspection path
    /// (`AgentCore::reach` walks this for the observer).
    pub bindings: Vec<ResolvedBinding>,
    pub sessions: crate::agent::types::Sessions,
    /// Internal memory store — replaces the old `memory`
    /// plugin dependency.
    pub memory: Arc<dyn MemoryBackend>,
}

impl AgentRuntime {
    pub fn new(
        cspace: CapabilitySpace,
        typed_bindings: TypedBindings,
        bindings: Vec<ResolvedBinding>,
    ) -> Self {
        Self {
            cspace,
            typed_bindings,
            bindings,
            sessions: global_sessions(),
            memory: pick_backend(),
        }
    }

    /// Allocate a new session and stash it in the shared table.
    pub fn start(
        &self,
        goal: String,
        context: Value,
        allowed_tools: Vec<String>,
        limits: SessionLimits,
    ) -> Result<SessionId, AgentError> {
        for tool in &allowed_tools {
            if self.cspace.lookup_by_name(tool).is_none() {
                return Err(AgentError::ToolUnknown(tool.clone()));
            }
        }
        let slots = AgentSlots::from_typed_bindings(&self.cspace, &self.typed_bindings);
        let session = Session::new(goal, context, allowed_tools, limits, slots, &self.cspace);
        let id = session.id.clone();
        self.sessions
            .lock()
            .expect("sessions poisoned")
            .insert(id.clone(), session);
        Ok(id)
    }

    pub fn lookup(&self, id: &str) -> Result<SessionId, AgentError> {
        let sid = SessionId(id.to_string());
        let sessions = self.sessions.lock().expect("sessions poisoned");
        let session = sessions
            .get(&sid)
            .ok_or_else(|| AgentError::UnknownSession(id.to_string()))?;
        match session.status {
            SessionStatus::Done | SessionStatus::Failed | SessionStatus::Cancelled => {
                Err(AgentError::SessionTerminal {
                    id: id.to_string(),
                    status: session.status.as_str().into(),
                })
            }
            _ => Ok(sid),
        }
    }

    pub fn session_event_bus_slot_id(&self, id: &str) -> Option<SlotId> {
        let sid = SessionId(id.to_string());
        let sessions = self.sessions.lock().expect("sessions poisoned");
        sessions.get(&sid).map(|s| s.event_bus_slot_id)
    }

    pub fn advance(
        &self,
        id: &str,
        observation: Observation,
    ) -> Result<(Step, Value, SessionStatus), AgentError> {
        let sid = SessionId(id.to_string());
        let mut sessions = self.sessions.lock().expect("sessions poisoned");
        let session = sessions
            .get_mut(&sid)
            .ok_or_else(|| AgentError::UnknownSession(id.to_string()))?;

        session.check_limits()?;
        session.last_advance_ms = Session::now_ms();
        session.step_count += 1;

        let first_action = matches!(observation, Observation::Tick);
        let prompt = build_prompt(session, &observation);
        let tool_schemas = collect_tool_schemas(&self.cspace, &session.allowed_tools);
        let llm_input = json!({
            "prompt": prompt,
            "system": build_system_prompt(first_action, &session.allowed_tools),
            "temperature": 0.7,
            "tools": tool_schemas,
            "event_bus_slot_id": session.event_bus_slot_id.raw(),
        });
        let llm_resp = session
            .slots
            .generator
            .invoke(Rights::INVOKE, llm_input)
            .map_err(|e| AgentError::LlmFailed(e.to_string()))?;

        let cspace_ref = self.cspace.clone();
        let (step, status) = if let Some(invocation) = first_native_tool_call(&llm_resp) {
            if let Some(prose) = llm_resp.get("text").and_then(Value::as_str)
                && !prose.is_empty()
            {
                session
                    .history
                    .push(Step::LlmReply(crate::agent::types::LlmReply::Text(
                        prose.to_string(),
                    )));
            }
            if !session.allowed_tools.is_empty()
                && !session.allowed_tools.contains(&invocation.tool)
            {
                return Err(AgentError::ToolDenied {
                    tool: invocation.tool,
                    reason: "not in allowed_tools",
                });
            }
            let cap = cspace_ref
                .lookup_by_name(&invocation.tool)
                .ok_or_else(|| AgentError::ToolUnknown(invocation.tool.clone()))?;
            let outcome = cap
                .invoke_dyn_typed(Rights::INVOKE, invocation.args.clone())
                .map_err(|e| AgentError::ToolFailed {
                    tool: invocation.tool.clone(),
                    error: e.to_string(),
                });
            let tr = ToolResult {
                tool: invocation.tool.clone(),
                outcome: outcome.map_err(|e| e.to_string()),
            };
            session.history.push(Step::ToolCall(invocation));
            session.history.push(Step::ToolResult(tr.clone()));
            (Step::ToolResult(tr), SessionStatus::AwaitingObservation)
        } else {
            let text = llm_resp
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| AgentError::LlmFailed("missing `text`".into()))?
                .to_string();
            parse_llm_reply(
                &text,
                &mut session.history,
                &session.allowed_tools,
                &cspace_ref,
            )?
        };

        if let Step::ToolResult(ref tr) = step {
            if let Some(prose) = llm_resp.get("text").and_then(Value::as_str)
                && !prose.is_empty()
            {
                session.push_event(AgentEvent::LlmReplyText(prose.to_string()));
            }
            if let Some(inv) = first_native_tool_call(&llm_resp) {
                session.push_event(AgentEvent::ToolCall(ToolInvocation {
                    tool: inv.tool.clone(),
                    args: inv.args.clone(),
                }));
            }
            session.push_event(AgentEvent::ToolResult(tr.clone()));
            session.status = status.clone();
            let history_value = session.history.to_value();
            return Ok((Step::ToolResult(tr.clone()), history_value, status));
        }
        session.push_event(match &step {
            Step::LlmReply(crate::agent::types::LlmReply::Text(t)) => {
                AgentEvent::LlmReplyText(t.clone())
            }
            Step::LlmReply(crate::agent::types::LlmReply::ToolCall(inv)) => {
                AgentEvent::ToolCall(inv.clone())
            }
            Step::LlmReply(crate::agent::types::LlmReply::Final(f)) => AgentEvent::Final(f.clone()),
            Step::ToolCall(inv) => AgentEvent::ToolCall(inv.clone()),
            Step::ToolResult(r) => AgentEvent::ToolResult(r.clone()),
            Step::Final(f) => AgentEvent::Final(f.clone()),
        });
        session.status = status.clone();
        let history_value = session.history.to_value();
        Ok((step, history_value, status))
    }

    pub fn cancel(&self, id: &str, path: Option<&str>) -> Result<Value, AgentError> {
        let sid = SessionId(id.to_string());
        let mut sessions = self.sessions.lock().expect("sessions poisoned");
        let mut session = sessions
            .remove(&sid)
            .ok_or_else(|| AgentError::UnknownSession(id.to_string()))?;
        session.status = SessionStatus::Cancelled;
        let bus_slot_id = session.event_bus_slot_id;
        drop(sessions);
        self.cspace.revoke(bus_slot_id);
        if let Some(p) = path {
            let checkpoint = SessionCheckpoint::from_session(&session, &sid);
            if let Some(parent) = std::path::Path::new(p).parent()
                && !parent.as_os_str().is_empty()
            {
                let _ = std::fs::create_dir_all(parent);
            }
            let bytes = serde_json::to_vec_pretty(&checkpoint).map_err(|e| {
                AgentError::Serialization(format!("session checkpoint serialise: {e}"))
            })?;
            std::fs::write(p, &bytes)
                .map_err(|e| AgentError::Serialization(format!("session checkpoint write: {e}")))?;
        }
        Ok(session.history.to_value())
    }

    pub fn load(&self, path: &str) -> Result<String, AgentError> {
        let bytes = std::fs::read(path)
            .map_err(|e| AgentError::Serialization(format!("session checkpoint read: {e}")))?;
        let checkpoint: SessionCheckpoint = serde_json::from_slice(&bytes)
            .map_err(|e| AgentError::Serialization(format!("session checkpoint parse: {e}")))?;
        let sid = SessionId(checkpoint.id.0.clone());
        let slots = AgentSlots::from_typed_bindings(&self.cspace, &self.typed_bindings);
        let (sender, _rx) = tokio::sync::broadcast::channel(64);
        let event_bus_slot_id =
            crate::agent::session::mint_session_event_bus(&self.cspace, sender.clone());
        let now = Session::now_ms();
        let session = Session {
            id: sid.clone(),
            goal: checkpoint.goal,
            context: checkpoint.context,
            allowed_tools: checkpoint.allowed_tools,
            history: checkpoint.history,
            status: checkpoint.status,
            limits: checkpoint.limits,
            created_at_ms: checkpoint.created_at_ms,
            step_count: checkpoint.step_count,
            last_advance_ms: checkpoint.last_advance_ms.max(now),
            slots,
            sender,
            event_bus_slot_id,
        };
        let mut sessions = self.sessions.lock().expect("sessions poisoned");
        if sessions.contains_key(&sid) {
            return Err(AgentError::Serialization(format!(
                "session `{sid}` already exists; load rejected to avoid clobbering"
            )));
        }
        sessions.insert(sid.clone(), session);
        Ok(sid.to_string())
    }

    pub fn plan(&self, goal: &str, tools: &[String]) -> Result<Value, AgentError> {
        let slots = AgentSlots::from_typed_bindings(&self.cspace, &self.typed_bindings);
        let system = build_plan_prompt(tools);
        let resp = slots
            .generator
            .invoke(
                Rights::INVOKE,
                json!({
                    "prompt": goal,
                    "system": system,
                    "temperature": 0.5,
                }),
            )
            .map_err(|e| AgentError::LlmFailed(e.to_string()))?;
        let text = resp
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| AgentError::LlmFailed("missing `text`".into()))?
            .to_string();
        Ok(json!({ "steps": [{"kind":"llm_text","text": text}] }))
    }

    /// Recall: embed the query, then run a vector search
    /// over the internal memory store.
    pub fn recall(
        &self,
        query: &str,
        top_k: usize,
        filter_tags: Vec<String>,
    ) -> Result<Value, AgentError> {
        let slots = AgentSlots::from_typed_bindings(&self.cspace, &self.typed_bindings);
        let embed_resp = slots
            .embedder
            .invoke(Rights::INVOKE, json!({ "text": query }))
            .map_err(|e| AgentError::LlmFailed(e.to_string()))?;
        let vector: Option<Vec<f32>> =
            embed_resp
                .get("vector")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_f64)
                        .map(|n| n as f32)
                        .collect()
                });
        let vector = vector.ok_or_else(|| AgentError::LlmFailed("missing `vector`".into()))?;
        let mut input = json!({
            "vector": vector,
            "top_k": top_k,
        });
        if !filter_tags.is_empty() {
            input["filter"] = json!({ "tags": filter_tags });
        }
        let resp = query_records(
            self.memory.as_ref(),
            None,
            input
                .get("vector")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_f64)
                        .map(|n| n as f32)
                        .collect::<Vec<f32>>()
                })
                .as_deref(),
            top_k,
            input
                .get("filter")
                .and_then(|f| f.get("tags"))
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_str)
                        .map(|s| s.to_string())
                        .collect::<Vec<String>>()
                })
                .as_deref()
                .unwrap_or(&[]),
        )
        .map_err(AgentError::MemoryFailed)?;
        Ok(resp)
    }

    /// Record: embed the content, then store in the internal
    /// memory store with the embedding attached.
    pub fn record(&self, content: Value, tags: Vec<String>) -> Result<Value, AgentError> {
        let slots = AgentSlots::from_typed_bindings(&self.cspace, &self.typed_bindings);
        let text_repr = content.to_string();
        let embed_resp = slots
            .embedder
            .invoke(Rights::INVOKE, json!({ "text": text_repr }))
            .map_err(|e| AgentError::LlmFailed(e.to_string()))?;
        let vector: Option<Vec<f32>> =
            embed_resp
                .get("vector")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_f64)
                        .map(|n| n as f32)
                        .collect()
                });
        let mut input = json!({ "content": content, "tags": tags });
        if let Some(v) = vector {
            input["vector"] = json!(v);
        }
        let resp = insert_record(self.memory.as_ref(), input).map_err(AgentError::MemoryFailed)?;
        Ok(resp)
    }
}
