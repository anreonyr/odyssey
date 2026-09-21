//! Agent data types — the wire/serialisable shapes that flow
//! through `Step`, `Observation`, `AgentEvent`, etc.
//!
//! `AgentError` here is the runtime's error type
//! (the observer has its own at `agent::core::AgentError`);
//! both are `pub`, accessible by full path. The agent's
//! internal code paths use whichever is closest.
//!
//! Cap names live as constants (`NAME_*`/`CONTRACT_*`/`REACHES`).
//! The plugin itself is `"agent"`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::model::embedder::EmbedderResource;
use crate::model::generator::GeneratorResource;

// ---------------------------------------------------------------------------
// Public names
// ---------------------------------------------------------------------------

pub const CONTRACT_START: &str = "agent_start";
pub const CONTRACT_RESUME: &str = "agent_resume";
pub const CONTRACT_CANCEL: &str = "agent_cancel";
pub const CONTRACT_PLAN: &str = "agent_plan";
pub const CONTRACT_STREAM: &str = "agent_stream";
pub const CONTRACT_RECALL: &str = "agent_memory_recall";
pub const CONTRACT_RECORD: &str = "agent_memory_record";
pub const CONTRACT_LOAD: &str = "agent_load";

pub const NAME_START: &str = "agent_start";
pub const NAME_RESUME: &str = "agent_resume";
pub const NAME_CANCEL: &str = "agent_cancel";
pub const NAME_PLAN: &str = "agent_plan";
pub const NAME_STREAM: &str = "agent_stream";
pub const NAME_LOAD: &str = "agent_load";
pub const NAME_RECALL: &str = "agent_memory_recall";
pub const NAME_RECORD: &str = "agent_memory_record";

/// The two providers the agent requires. Each tuple is
/// `(local_handle, contract_name)`; the contract name must match
/// the corresponding `CapabilityDecl::contract_name` in the
/// `model/generator` and `model/embedder` plugin manifests.
pub const REACHES: [(&str, &str); 2] = [("generator", "generator"), ("embedder", "embedder")];

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum AgentError {
    Input(String),
    UnknownSession(String),
    SessionTerminal {
        id: String,
        status: String,
    },
    SessionLimit {
        id: String,
        limit: String,
        observed: u32,
    },
    NoLlm,
    NoMemory,
    ToolDenied {
        tool: String,
        reason: &'static str,
    },
    ToolUnknown(String),
    ToolFailed {
        tool: String,
        error: String,
    },
    LlmFailed(String),
    MemoryFailed(String),
    SchemaMissing(String),
    Serialization(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(m) => write!(f, "agent: {m}"),
            Self::UnknownSession(id) => write!(f, "agent: unknown session `{id}`"),
            Self::SessionTerminal { id, status } => {
                write!(f, "agent: session `{id}` is terminal ({status})")
            }
            Self::SessionLimit {
                id,
                limit,
                observed,
            } => {
                write!(
                    f,
                    "agent: session `{id}` exceeded {limit} (observed: {observed})"
                )
            }
            Self::NoLlm => write!(f, "agent: LLM cap not bound"),
            Self::NoMemory => write!(f, "agent: memory cap not bound"),
            Self::ToolDenied { tool, reason } => {
                write!(f, "agent: tool `{tool}` denied ({reason})")
            }
            Self::ToolUnknown(t) => write!(f, "agent: tool `{t}` not found in cspace"),
            Self::ToolFailed { tool, error } => {
                write!(f, "agent: tool `{tool}` failed: {error}")
            }
            Self::LlmFailed(m) => write!(f, "agent: LLM failed: {m}"),
            Self::MemoryFailed(m) => write!(f, "agent: memory failed: {m}"),
            Self::SchemaMissing(t) => {
                write!(f, "agent: tool `{t}` has no `tool_schema` declared")
            }
            Self::Serialization(m) => write!(f, "agent: serialization: {m}"),
        }
    }
}

// ---------------------------------------------------------------------------
// Session types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        Self(format!("sess_{now}"))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Running,
    AwaitingObservation,
    Done,
    Failed,
    Cancelled,
}

impl SessionStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::AwaitingObservation => "AwaitingObservation",
            Self::Done => "Done",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Step {
    LlmReply(LlmReply),
    ToolCall(ToolInvocation),
    ToolResult(ToolResult),
    Final(FinalAnswer),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LlmReply {
    Text(String),
    ToolCall(ToolInvocation),
    Final(FinalAnswer),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInvocation {
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool: String,
    pub outcome: Result<Value, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalAnswer {
    pub value: Value,
    pub reason: FinalReason,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FinalReason {
    Goal,
    MaxSteps,
    ToolFailure,
}

#[derive(Debug, Clone)]
pub enum Observation {
    Tick,
    ToolResult {
        tool: String,
        value: Value,
        error: Option<String>,
    },
    UserReply(String),
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    LlmReplyText(String),
    /// A single streamed text fragment from a real LLM
    /// (OpenAI `delta.content`). The `Generator` backend
    /// emits one of these for every streaming delta. The
    /// `LlmReplyText` variant keeps its role as the
    /// final/non-streamed text reply.
    LlmDelta(String),
    ToolCall(ToolInvocation),
    ToolResult(ToolResult),
    Final(FinalAnswer),
    Error(String),
}

impl AgentEvent {
    pub fn to_value(&self) -> Value {
        match self {
            Self::LlmReplyText(t) => json!({ "kind": "llm_reply_text", "text": t }),
            Self::LlmDelta(s) => json!({ "kind": "llm_delta", "text": s }),
            Self::ToolCall(inv) => json!({
                "kind": "tool_call",
                "tool": inv.tool,
                "args": inv.args,
            }),
            Self::ToolResult(r) => {
                let outcome = match &r.outcome {
                    Ok(v) => json!({ "ok": true, "value": v }),
                    Err(e) => json!({ "ok": false, "error": e }),
                };
                json!({
                    "kind": "tool_result",
                    "tool": r.tool,
                    "outcome": outcome,
                })
            }
            Self::Final(f) => json!({
                "kind": "final",
                "value": f.value,
                "reason": format!("{:?}", f.reason),
            }),
            Self::Error(m) => json!({ "kind": "error", "message": m }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLimits {
    pub max_steps: u32,
    pub max_idle_ms: u32,
    pub max_session_ms: u32,
}

impl Default for SessionLimits {
    fn default() -> Self {
        Self {
            max_steps: 30,
            max_idle_ms: 300_000,
            max_session_ms: 1_800_000,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub steps: Vec<Step>,
}

impl History {
    pub fn push(&mut self, step: Step) {
        self.steps.push(step);
    }
    pub fn iter(&self) -> std::slice::Iter<'_, Step> {
        self.steps.iter()
    }
    pub fn to_value(&self) -> Value {
        Value::Array(self.steps.iter().map(step_to_value).collect())
    }
}

pub fn step_to_value(s: &Step) -> Value {
    match s {
        Step::LlmReply(LlmReply::Text(t)) => json!({ "kind": "llm_text", "text": t }),
        Step::LlmReply(LlmReply::ToolCall(inv)) => json!({
            "kind": "llm_tool_call", "tool": inv.tool, "args": inv.args
        }),
        Step::LlmReply(LlmReply::Final(f)) => json!({
            "kind": "llm_final", "value": f.value, "reason": format!("{:?}", f.reason)
        }),
        Step::ToolCall(inv) => json!({ "kind": "tool_call", "tool": inv.tool, "args": inv.args }),
        Step::ToolResult(r) => {
            let outcome = match &r.outcome {
                Ok(v) => json!({ "ok": true, "value": v }),
                Err(e) => json!({ "ok": false, "error": e }),
            };
            json!({ "kind": "tool_result", "tool": r.tool, "outcome": outcome })
        }
        Step::Final(f) => json!({
            "kind": "final", "value": f.value, "reason": format!("{:?}", f.reason)
        }),
    }
}

// ---------------------------------------------------------------------------
// Slot holder + Session
// ---------------------------------------------------------------------------

/// Typed handles the agent looks up once per session, when
/// the orchestrator hands the runtime a `TypedBindings`.
///
/// After the refactor: just `generator` and `embedder`.
/// Memory moved internal; no slot needed.
pub struct AgentSlots {
    pub generator: odyssey::capability::handle::slot::Slot<GeneratorResource>,
    pub embedder: odyssey::capability::handle::slot::Slot<EmbedderResource>,
}

impl AgentSlots {
    pub fn from_typed_bindings(
        cspace: &odyssey::capability::enforce::space::CapabilitySpace,
        typed_bindings: &odyssey::personality::lifecycle::mint::TypedBindings,
    ) -> Self {
        use odyssey::capability::handle::slot::Slot;
        let lookup = |handle: &str| -> odyssey::core::identity::ids::SlotId {
            typed_bindings
                .entries
                .iter()
                .find(|e| e.handle == handle)
                .map(|e| e.slot_id)
                .unwrap_or_else(|| panic!("agent_runtime: missing typed binding for `{handle}`"))
        };
        let gen_slot: Slot<GeneratorResource> = Slot::new(cspace.clone(), lookup("generator"));
        let emb_slot: Slot<EmbedderResource> = Slot::new(cspace.clone(), lookup("embedder"));
        Self {
            generator: gen_slot,
            embedder: emb_slot,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionCheckpoint {
    pub id: SessionId,
    pub goal: String,
    pub context: Value,
    pub allowed_tools: Vec<String>,
    pub history: History,
    pub status: SessionStatus,
    pub limits: SessionLimits,
    pub created_at_ms: u128,
    pub step_count: u32,
    pub last_advance_ms: u128,
}

impl SessionCheckpoint {
    pub fn from_session(s: &Session, id: &SessionId) -> Self {
        Self {
            id: id.clone(),
            goal: s.goal.clone(),
            context: s.context.clone(),
            allowed_tools: s.allowed_tools.clone(),
            history: s.history.clone(),
            status: s.status.clone(),
            limits: s.limits.clone(),
            created_at_ms: s.created_at_ms,
            step_count: s.step_count,
            last_advance_ms: s.last_advance_ms,
        }
    }
}

pub struct Session {
    pub id: SessionId,
    pub goal: String,
    pub context: Value,
    pub allowed_tools: Vec<String>,
    pub history: History,
    pub status: SessionStatus,
    pub limits: SessionLimits,
    pub created_at_ms: u128,
    pub step_count: u32,
    pub last_advance_ms: u128,
    pub slots: AgentSlots,
    pub sender: broadcast::Sender<AgentEvent>,
    pub event_bus_slot_id: odyssey::core::identity::ids::SlotId,
}

impl Session {
    pub fn now_ms() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    }

    pub fn new(
        goal: String,
        context: Value,
        allowed_tools: Vec<String>,
        limits: SessionLimits,
        slots: AgentSlots,
        cspace: &odyssey::capability::enforce::space::CapabilitySpace,
    ) -> Self {
        let id = SessionId::new();
        let (sender, _rx) = broadcast::channel(64);
        let event_bus_slot_id =
            crate::agent::session::mint_session_event_bus(cspace, sender.clone());
        Self {
            id,
            goal,
            context,
            allowed_tools,
            history: History::default(),
            status: SessionStatus::Running,
            limits,
            created_at_ms: Self::now_ms(),
            step_count: 0,
            last_advance_ms: Self::now_ms(),
            slots,
            sender,
            event_bus_slot_id,
        }
    }

    pub fn check_limits(&self) -> Result<(), AgentError> {
        if self.step_count >= self.limits.max_steps {
            return Err(AgentError::SessionLimit {
                id: self.id.to_string(),
                limit: "max_steps".to_string(),
                observed: self.step_count,
            });
        }
        let now = Self::now_ms();
        let idle = now.saturating_sub(self.last_advance_ms);
        if idle > self.limits.max_idle_ms as u128 {
            return Err(AgentError::SessionLimit {
                id: self.id.to_string(),
                limit: "max_idle_ms".to_string(),
                observed: idle as u32,
            });
        }
        let lifetime = now.saturating_sub(self.created_at_ms);
        if lifetime > self.limits.max_session_ms as u128 {
            return Err(AgentError::SessionLimit {
                id: self.id.to_string(),
                limit: "max_session_ms".to_string(),
                observed: lifetime as u32,
            });
        }
        Ok(())
    }

    pub fn push_event(&self, ev: AgentEvent) {
        let _ = self.sender.send(ev);
    }
}

// Convenience alias for the global sessions table.
pub type Sessions = Arc<Mutex<HashMap<SessionId, Session>>>;
