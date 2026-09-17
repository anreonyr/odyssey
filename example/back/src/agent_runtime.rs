//! Agent runtime builtin — seven capabilities that drive a
//! caller-controlled ReAct loop.
//!
//! | cap | kind | purpose |
//! |---|---|---|
//! | `agent_start` | sync | Create a session, lock the tool whitelist, return `session_id` + profile |
//! | `agent_resume` | sync | Drive one ReAct step (LLM call + optional tool call) |
//! | `agent_cancel` | sync | Destroy a session, return final history |
//! | `agent_plan` | sync | Pure planning, no session, no tool calls |
//! | `agent_stream` | stream | Subscribe to a session's event log (read-only) |
//! | `agent_memory_recall` | sync | Embed query → search memory |
//! | `agent_memory_record` | sync | Embed content → store in memory |
//!
//! ## Why this is a separate plugin
//!
//! The existing `agent` plugin exposes two read-only observers
//! (`agent_list`, `agent_describe`) over the *binding table*. This
//! plugin exposes the runtime caps that *use* the agent's LLM and
//! memory to actually think and act. Splitting them keeps each
//! manifest's `requires` honest: the observers require
//! `echo`/`reverse`/`database`/`streaming_echo` (the tool caps);
//! the runtime requires `llm_complete`/`llm_embed`/`memory_query`/
//! `memory_insert` (the agent's providers). The existing
//! `agent_manifest_requires_resolve_to_a_binding_row_for_every_handle`
//! test pins the old shape; the runtime caps don't disturb it.
//!
//! ## Session model
//!
//! Sessions are stored in a process-global `OnceLock<Mutex<HashMap>>`.
//! Each `agent_*` cap constructs a fresh `AgentRuntime` on every
//! mint, but the runtime holds an `Arc` to the shared sessions
//! table. A session is identified by an opaque `SessionId` string.
//! `agent_start` allocates; `agent_resume` advances; `agent_cancel`
//! drops the session (which closes its event broadcast).
//!
//! ## Event model
//!
//! Each session carries a `broadcast::Sender<AgentEvent>`. The
//! runtime pushes events as it advances. `agent_stream` opens a
//! `broadcast::Receiver` on the session, forwards events through a
//! `mpsc::Sender<CapabilityChunk>`, and emits `CapabilityChunk::Done`
//! when the broadcast closes (which happens when `agent_cancel`
//! drops the session, closing the broadcast).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::llm::{LlmCompleteResource, LlmEmbedResource};
use crate::memory::{MemoryInsertResource, MemoryQueryResource};
use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::capability::handle::cap::Capability;
use odyssey::capability::handle::slot::Slot;
use odyssey::core::Resource;
use odyssey::core::clock::clock::SystemClock;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{CapabilityId, PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::core::meta::meta::CapabilityMeta;
use odyssey::core::rights::rights::{CapabilityRights, Rights};
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};
use tokio::sync::broadcast;

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

/// The four providers the agent requires. Each tuple is
/// `(local_handle, contract_name)`; the contract name must match
/// the corresponding `CapabilityDecl::contract_name` in the
/// `llm` and `memory` plugin manifests.
pub const REACHES: [(&str, &str); 4] = [
    ("llm_complete", "llm_complete"),
    ("llm_embed", "llm_embed"),
    ("memory_query", "memory_query"),
    ("memory_insert", "memory_insert"),
];

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

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SessionId(String);

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Step {
    LlmReply(LlmReply),
    ToolCall(ToolInvocation),
    ToolResult(ToolResult),
    Final(FinalAnswer),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum LlmReply {
    Text(String),
    ToolCall(ToolInvocation),
    Final(FinalAnswer),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolInvocation {
    pub tool: String,
    pub args: Value,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ToolResult {
    pub tool: String,
    pub outcome: Result<Value, String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FinalAnswer {
    pub value: Value,
    pub reason: FinalReason,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum FinalReason {
    Goal,
    MaxSteps,
    ToolFailure,
}

/// What the caller hands the agent to advance the loop.
#[derive(Debug, Clone)]
pub enum Observation {
    /// First call to a freshly-started session. The LLM is
    /// asked for the first action.
    Tick,
    /// The caller executed a tool the agent requested and is
    /// feeding the result back.
    ToolResult {
        tool: String,
        value: Value,
        error: Option<String>,
    },
    /// Free-form user input. The agent appends it to history
    /// and asks the LLM for the next action.
    UserReply(String),
}

#[derive(Debug, Clone)]
pub enum AgentEvent {
    LlmReplyText(String),
    /// A single streamed text fragment from a real LLM
    /// (OpenAI `delta.content`). The `LlmCompleteResource`
    /// emits one of these for every `StreamEvent::Delta` it
    /// receives during `complete_stream`. Subscribers to
    /// `agent_stream` see them as they arrive (server-side
    /// events; the LLM cap is on the same thread, so the
    /// pushes happen during the call). The existing
    /// `LlmReplyText` variant keeps its role as the
    /// final/non-streamed text reply, with a tool call
    /// attached when the LLM emits one.
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
                json!({ "kind": "tool_result", "tool": r.tool, "outcome": outcome })
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

/// Newtype around `Vec<Step>` to make the append-only
/// invariant a type obligation: only `push()` is public, no
/// indexing assignment, no clear, no remove.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct History {
    steps: Vec<Step>,
}

impl History {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }
    pub fn push(&mut self, step: Step) {
        self.steps.push(step);
    }
    pub fn len(&self) -> usize {
        self.steps.len()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, Step> {
        self.steps.iter()
    }
    pub fn to_value(&self) -> Value {
        Value::Array(
            self.steps
                .iter()
                .map(|s| match s {
                    Step::LlmReply(LlmReply::Text(t)) => json!({"kind":"llm_text","text":t}),
                    Step::LlmReply(LlmReply::ToolCall(inv)) => json!({
                        "kind":"llm_tool_call","tool":inv.tool,"args":inv.args
                    }),
                    Step::LlmReply(LlmReply::Final(f)) => json!({
                        "kind":"llm_final","value":f.value,"reason":format!("{:?}",f.reason)
                    }),
                    Step::ToolCall(inv) => {
                        json!({"kind":"tool_call","tool":inv.tool,"args":inv.args})
                    }
                    Step::ToolResult(r) => {
                        let outcome = match &r.outcome {
                            Ok(v) => json!({"ok":true,"value":v}),
                            Err(e) => json!({"ok":false,"error":e}),
                        };
                        json!({"kind":"tool_result","tool":r.tool,"outcome":outcome})
                    }
                    Step::Final(f) => json!({
                        "kind":"final","value":f.value,"reason":format!("{:?}",f.reason)
                    }),
                })
                .collect(),
        )
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, Copy)]
pub enum SessionLimit {
    Steps,
    Idle,
    Wall,
}

impl SessionLimit {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Steps => "max_steps",
            Self::Idle => "max_idle_ms",
            Self::Wall => "max_session_ms",
        }
    }
}

/// A snapshot of the typed slots the agent holds. Cheap to
/// clone (Slot is an Arc-share under the hood).
#[derive(Clone)]
pub struct AgentSlots {
    pub llm_complete: Slot<LlmCompleteResource>,
    pub llm_embed: Slot<LlmEmbedResource>,
    pub memory_query: Slot<MemoryQueryResource>,
    pub memory_insert: Slot<MemoryInsertResource>,
}

impl AgentSlots {
    /// Build the four typed slots from cspace + bindings. The
    /// `ResolvedBinding` rows name the capability each `requires`
    /// resolved to. The slot id comes from the live cap.
    pub fn from_bindings(cspace: &CapabilitySpace, bindings: &[ResolvedBinding]) -> Self {
        let slot_for = |handle: &str| -> SlotId {
            let b = bindings
                .iter()
                .find(|b| b.handle == handle)
                .unwrap_or_else(|| panic!("agent_runtime: required handle `{handle}` missing"));
            cspace.slot_for_name(&b.capability).unwrap_or_else(|| {
                panic!("agent_runtime: cap `{}` not bound to a slot", b.capability)
            })
        };

        Self {
            llm_complete: Slot::new(cspace.clone(), slot_for("llm_complete")),
            llm_embed: Slot::new(cspace.clone(), slot_for("llm_embed")),
            memory_query: Slot::new(cspace.clone(), slot_for("memory_query")),
            memory_insert: Slot::new(cspace.clone(), slot_for("memory_insert")),
        }
    }
}

/// Serialisable snapshot of a session's state. Excludes
/// the runtime-only fields (typed `AgentSlots`, broadcast
/// `sender`, the `id` is preserved verbatim so external
/// bookkeeping survives a save/load round-trip). The file
/// format is pretty-printed JSON; human-readable, diff-
/// friendly, and forward-compatible (new optional fields
/// can be added without breaking old loaders via
/// `#[serde(default)]` on the consumer).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionCheckpoint {
    pub id: SessionId,
    pub goal: String,
    pub context: Value,
    pub allowed_tools: Vec<String>,
    pub history: History,
    pub status: SessionStatus,
    pub limits: SessionLimits,
    pub created_at_ms: u64,
    pub step_count: u32,
    pub last_advance_ms: u64,
}

impl SessionCheckpoint {
    /// Build a checkpoint from a live session. The session
    /// is borrowed; the caller still owns it and decides
    /// when to remove it from the global table.
    pub fn from_session(session: &Session, sid: &SessionId) -> Self {
        Self {
            id: sid.clone(),
            goal: session.goal.clone(),
            context: session.context.clone(),
            allowed_tools: session.allowed_tools.clone(),
            history: session.history.clone(),
            status: session.status.clone(),
            limits: session.limits.clone(),
            created_at_ms: session.created_at_ms,
            step_count: session.step_count,
            last_advance_ms: session.last_advance_ms,
        }
    }
}

/// One in-flight agent session. The `sender` is the broadcast
/// handle `agent_stream` subscribers read from; the
/// `AgentSlots` are typed slot refs to the LLM and memory
/// caps the agent uses during a step.
///
/// `event_bus_slot_id` is the slot id of this session's
/// `SessionEventBusResource` in the global cspace. The
/// `agent_stream` cap and the LLM plugin reach the
/// session's broadcast via this typed capability, not via
/// the static `SESSIONS` table. The slot id is the only
/// reachability handle the plugins see — they never learn
/// the session id directly, and the static SESSIONS table
/// is now an internal implementation detail of the
/// agent_runtime module.
pub struct Session {
    pub id: SessionId,
    pub goal: String,
    pub context: Value,
    pub allowed_tools: Vec<String>,
    pub history: History,
    pub status: SessionStatus,
    pub limits: SessionLimits,
    pub created_at_ms: u64,
    pub step_count: u32,
    pub last_advance_ms: u64,
    pub slots: AgentSlots,
    pub sender: broadcast::Sender<AgentEvent>,
    pub event_bus_slot_id: SlotId,
}

impl Session {
    fn new(
        goal: String,
        context: Value,
        allowed_tools: Vec<String>,
        limits: SessionLimits,
        slots: AgentSlots,
        cspace: &CapabilitySpace,
    ) -> Self {
        let id = SessionId::new();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let (sender, _rx) = broadcast::channel(64);
        // Mint the per-session event bus cap into the global
        // cspace. The slot id is the only handle external
        // plugins see; they reach the session's broadcast
        // through this typed cap, not via the static
        // SESSIONS table.
        let event_bus_slot_id = mint_session_event_bus(cspace, sender.clone());
        Self {
            id,
            goal,
            context,
            allowed_tools,
            history: History::new(),
            status: SessionStatus::Running,
            limits,
            created_at_ms: now,
            step_count: 0,
            last_advance_ms: now,
            slots,
            sender,
            event_bus_slot_id,
        }
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    fn check_limits(&self) -> Result<(), AgentError> {
        if self.step_count >= self.limits.max_steps {
            return Err(AgentError::SessionLimit {
                id: self.id.to_string(),
                limit: SessionLimit::Steps.as_str().into(),
                observed: self.step_count,
            });
        }
        let now = Self::now_ms();
        if now.saturating_sub(self.last_advance_ms) > self.limits.max_idle_ms as u64 {
            return Err(AgentError::SessionLimit {
                id: self.id.to_string(),
                limit: SessionLimit::Idle.as_str().into(),
                observed: self.step_count,
            });
        }
        if now.saturating_sub(self.created_at_ms) > self.limits.max_session_ms as u64 {
            return Err(AgentError::SessionLimit {
                id: self.id.to_string(),
                limit: SessionLimit::Wall.as_str().into(),
                observed: self.step_count,
            });
        }
        Ok(())
    }

    fn push_event(&self, ev: AgentEvent) {
        // `send` returns Err only when there are no receivers.
        // We don't care; the session still records the event in
        // its history.
        let _ = self.sender.send(ev);
    }
}

// ---------------------------------------------------------------------------
// SessionEventBusResource — per-session typed event publisher
// ---------------------------------------------------------------------------

/// Typed capability that publishes `AgentEvent`s to a session's
/// broadcast. The LLM plugin (and future streaming tools) invoke
/// this resource to push per-token deltas, replacing the old
/// `pub fn push_event_to_session(sid, event)` cross-call into
/// `agent_runtime`'s static `SESSIONS` table.
///
/// Slice 4: the LLM plugin reaches the bus by slot id (the
/// agent_runtime hands it the slot id in the LLM input). The
/// plugin looks the slot up via the cspace and invokes through
/// the typed cap. The static `SESSIONS` table is no longer
/// reachable from the LLM plugin; `push_event_to_session` is
/// deleted in this commit.
pub struct SessionEventBusResource {
    sender: broadcast::Sender<AgentEvent>,
}

impl Resource for SessionEventBusResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        // The bus accepts the same JSON envelope that
        // `AgentEvent::to_value` produces. Round-tripping through
        // JSON keeps the cap boundary typed: the producer (LLM
        // plugin) constructs the JSON from a typed `AgentEvent`
        // value via `to_value`; the bus parses it back. We
        // dispatch on `kind` so the JSON shape stays
        // forward-compatible (unknown kinds surface as
        // `AgentEvent::Error`, never as silent drops).
        let kind = input
            .get("kind")
            .and_then(Value::as_str)
            .ok_or_else(|| "session_event_bus: missing `kind`".to_string())?;
        let event = match kind {
            "llm_delta" => AgentEvent::LlmDelta(
                input
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "session_event_bus(llm_delta): missing `text`".to_string())?
                    .to_string(),
            ),
            "llm_reply_text" => AgentEvent::LlmReplyText(
                input
                    .get("text")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "session_event_bus(llm_reply_text): missing `text`".to_string())?
                    .to_string(),
            ),
            "error" => AgentEvent::Error(
                input
                    .get("message")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "session_event_bus(error): missing `message`".to_string())?
                    .to_string(),
            ),
            // `tool_call` and `tool_result` are produced by
            // the agent_runtime itself (not the LLM), so the
            // bus doesn't currently need to handle them. They
            // would route to the session via `session.push_event`
            // directly. Surfacing unknown kinds explicitly so
            // a future variant doesn't silently drop.
            other => {
                return Err(format!(
                    "session_event_bus: unhandled kind `{other}` (LLM should only emit llm_delta / llm_reply_text / error)"
                ));
            }
        };
        // `send` returns Err only when there are no receivers.
        // The bus is a best-effort publisher; events with no
        // listener are dropped silently.
        let _ = self.sender.send(event);
        Ok(Value::Null)
    }
}

/// Mint a fresh `CapabilityId` for a session event bus. The
/// agent_runtime mints these at runtime (per-session), so they
/// don't share the factory's id counter.
fn next_bus_capability_id() -> CapabilityId {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    CapabilityId(NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Mint a `SessionEventBusResource` into `cspace` and return its
/// slot id. The cap's name is opaque — callers use the returned
/// slot id directly, never the name — but it's unique in the
/// cspace so it doesn't collide with user-facing caps.
fn mint_session_event_bus(
    cspace: &CapabilitySpace,
    sender: broadcast::Sender<AgentEvent>,
) -> SlotId {
    let slot_id = cspace.allocate();
    let meta = CapabilityMeta {
        id: next_bus_capability_id(),
        name: format!("__session_event_bus_{}", slot_id.raw()),
        namespace: "agent_runtime".into(),
        contract_name: "session_event_bus".into(),
        plugin: PluginId {
            name: "agent_runtime".into(),
            version: "0.1.0".into(),
        },
        kind: CapKind::Sync,
        timeout_ms: 1000,
        quota: Default::default(),
        tool_schema: None,
    };
    let rights = CapabilityRights {
        operations: Rights::INVOKE,
        timeout_ms: 1000,
    };
    let budget = CapabilityBudget::new(1000);
    let cap = Capability::new(
        meta,
        Arc::new(SessionEventBusResource { sender }),
        budget,
        rights,
        CapKind::Sync,
        Arc::new(SystemClock),
    );
    cspace.install(slot_id, Arc::new(cap));
    slot_id
}

// ---------------------------------------------------------------------------
// Global sessions table — shared across all agent_runtime caps
// ---------------------------------------------------------------------------

type Sessions = Arc<Mutex<HashMap<SessionId, Session>>>;

fn global_sessions() -> Sessions {
    static SESSIONS: OnceLock<Sessions> = OnceLock::new();
    SESSIONS
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

/// Subscribe to a session's event broadcast by id. Used by
/// tests and by the `agent_stream` cap to receive
/// per-token `LlmDelta`s. Returns `None` if the session
/// is unknown (it has been cancelled or the id is wrong).
pub fn subscribe_session_broadcast(session_id: &str) -> Option<broadcast::Receiver<AgentEvent>> {
    let sessions_arc = global_sessions();
    let sessions = sessions_arc.lock().expect("sessions poisoned");
    sessions
        .get(&SessionId(session_id.to_string()))
        .map(|s| s.sender.subscribe())
}

// ---------------------------------------------------------------------------
// AgentRuntime — the shared core the seven Resources hold
// ---------------------------------------------------------------------------

pub struct AgentRuntime {
    pub cspace: CapabilitySpace,
    pub bindings: Vec<ResolvedBinding>,
    pub sessions: Sessions,
}

impl AgentRuntime {
    pub fn new(cspace: CapabilitySpace, bindings: Vec<ResolvedBinding>) -> Self {
        Self {
            cspace,
            bindings,
            sessions: global_sessions(),
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
        let slots = AgentSlots::from_bindings(&self.cspace, &self.bindings);
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

    /// Slot id of the per-session `SessionEventBus` cap in
    /// the global cspace. Other plugins (the LLM plugin in
    /// particular) reach the session's broadcast via this
    /// slot id. Returns `None` if the session is unknown.
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
        // Build the native `tools` array. Each entry is the
        // tool name, a description, and a JSON Schema. The
        // OpenAI backend forwards this to the LLM as the
        // `tools` field of the chat-completions body; the
        // mock backend also honours it (returns a
        // `tool_call` for the first tool when present).
        // Backends that don't support native tool calling
        // simply ignore the field.
        let tool_schemas = collect_tool_schemas(&self.cspace, &session.allowed_tools);
        let llm_input = json!({
            "prompt": prompt,
            "system": build_system_prompt(first_action, &session.allowed_tools),
            "temperature": 0.7,
            "tools": tool_schemas,
            // Route per-token `Delta` events to this session's
            // broadcast via the session's `SessionEventBus`
            // capability. The LLM plugin holds a reference to
            // the cspace, looks up the bus by slot id, and
            // invokes through the typed cap (WRITE). The
            // bus pushes to the broadcast sender internally.
            // No `pub fn` cross-call into agent_runtime.
            "event_bus_slot_id": session.event_bus_slot_id.raw(),
        });
        let llm_resp = session
            .slots
            .llm_complete
            .invoke(Rights::INVOKE, llm_input)
            .map_err(|e| AgentError::LlmFailed(e.to_string()))?;

        // The LLM backend may return either:
        //   (a) native `tool_calls` (real LLMs), or
        //   (b) text content with JSON the agent's parser
        //       understands (the mock backend's path).
        // Prefer the native path. Fall back to the text
        // parser when `tool_calls` is empty.
        let cspace_ref = self.cspace.clone();
        let (step, status) = if let Some(invocation) = first_native_tool_call(&llm_resp) {
            // Native-tool-call path. The LLM often emits
            // both `content` (prose) and `tool_calls` in
            // the same message; the prose is explanatory
            // ("I'll call echo with …") and must be
            // preserved in history and the event stream.
            // Record it before the tool call so the
            // chronological order matches what the LLM
            // produced.
            if let Some(prose) = llm_resp.get("text").and_then(Value::as_str)
                && !prose.is_empty()
            {
                session
                    .history
                    .push(Step::LlmReply(LlmReply::Text(prose.to_string())));
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
            // Phase M3: the kernel enforces rights now. The
            // pre-M3 `ops.contains(EXECUTE)` plugin-side check
            // was needed because every entry point bypassed
            // `Capability::invoke`'s rights test. With
            // `invoke_dyn_typed` going through the with-check
            // path, this branch is unreachable; the kernel
            // surfaces `OperationDenied` as `Handler` error.
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

        // When the native path recorded the LLM's prose in
        // history above, mirror it on the event stream so
        // `agent_stream` subscribers see the same
        // chronological order. Three events, in order:
        // prose (if any), the tool call, the tool result.
        // The text branch pushes its own events via the
        // step→event mapping below.
        if let Step::ToolResult(ref tr) = step {
            if let Some(prose) = llm_resp.get("text").and_then(Value::as_str)
                && !prose.is_empty()
            {
                session.push_event(AgentEvent::LlmReplyText(prose.to_string()));
            }
            // The tool call itself is a separate event
            // from the result. `step` carries only the
            // result, so emit the matching ToolCall from
            // the invocation we stashed.
            if let Some(inv) = first_native_tool_call(&llm_resp) {
                session.push_event(AgentEvent::ToolCall(ToolInvocation {
                    tool: inv.tool.clone(),
                    args: inv.args.clone(),
                }));
            }
            session.push_event(AgentEvent::ToolResult(tr.clone()));
            // Skip the generic step→event mapping below —
            // we just emitted the events directly.
            session.status = status.clone();
            let history_value = session.history.to_value();
            return Ok((Step::ToolResult(tr.clone()), history_value, status));
        }
        session.push_event(match &step {
            Step::LlmReply(LlmReply::Text(t)) => AgentEvent::LlmReplyText(t.clone()),
            Step::LlmReply(LlmReply::ToolCall(inv)) => AgentEvent::ToolCall(inv.clone()),
            Step::LlmReply(LlmReply::Final(f)) => AgentEvent::Final(f.clone()),
            Step::ToolCall(inv) => AgentEvent::ToolCall(inv.clone()),
            Step::ToolResult(r) => AgentEvent::ToolResult(r.clone()),
            Step::Final(f) => AgentEvent::Final(f.clone()),
        });
        session.status = status.clone();
        let history_value = session.history.to_value();
        Ok((step, history_value, status))
    }

    /// Cancel a session. If `path` is provided, the session
    /// state is serialised to that file *before* the session
    /// is removed. The file can later be loaded with
    /// `AgentRuntime::load` to resurrect the session. The
    /// path's parent directory is created if it doesn't
    /// exist. The session's id is preserved on restore so
    /// external bookkeeping keeps working.
    pub fn cancel(&self, id: &str, path: Option<&str>) -> Result<Value, AgentError> {
        let sid = SessionId(id.to_string());
        let mut sessions = self.sessions.lock().expect("sessions poisoned");
        let mut session = sessions
            .remove(&sid)
            .ok_or_else(|| AgentError::UnknownSession(id.to_string()))?;
        session.status = SessionStatus::Cancelled;
        // Slice 4: revoke the per-session event bus cap so
        // its cloned `broadcast::Sender` drops. Without this
        // the bus keeps the broadcast alive past cancel
        // and `agent_stream` subscribers never see the
        // channel close. We revoke (single) — the bus has
        // no derived caps, only the LLM plugin holds the
        // slot id and uses it for invoke (not derivation).
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

    /// Load a session from a checkpoint file. The new
    /// session has the *same* id as the original (so
    /// external callers' records stay valid), fresh
    /// LLM/Memory slots pointing at the same cspace caps,
    /// and a fresh broadcast sender. Returns the new
    /// session id.
    pub fn load(&self, path: &str) -> Result<String, AgentError> {
        let bytes = std::fs::read(path)
            .map_err(|e| AgentError::Serialization(format!("session checkpoint read: {e}")))?;
        let checkpoint: SessionCheckpoint = serde_json::from_slice(&bytes)
            .map_err(|e| AgentError::Serialization(format!("session checkpoint parse: {e}")))?;
        let sid = SessionId(checkpoint.id.0.clone());
        let slots = AgentSlots::from_bindings(&self.cspace, &self.bindings);
        let (sender, _rx) = broadcast::channel(64);
        let event_bus_slot_id = mint_session_event_bus(&self.cspace, sender.clone());
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
        let slots = AgentSlots::from_bindings(&self.cspace, &self.bindings);
        let system = build_plan_prompt(tools);
        let resp = slots
            .llm_complete
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

    pub fn recall(
        &self,
        query: &str,
        top_k: usize,
        filter_tags: Vec<String>,
    ) -> Result<Value, AgentError> {
        let slots = AgentSlots::from_bindings(&self.cspace, &self.bindings);
        let embed_resp = slots
            .llm_embed
            .invoke(Rights::INVOKE, json!({ "texts": [query] }))
            .map_err(|e| AgentError::LlmFailed(e.to_string()))?;
        let vector: Option<Vec<f32>> = embed_resp
            .get("vectors")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_f64)
                    .map(|n| n as f32)
                    .collect()
            });
        let vector = vector.ok_or_else(|| AgentError::LlmFailed("missing vectors[0]".into()))?;
        let mut input = json!({
            "vector": vector,
            "top_k": top_k,
        });
        if !filter_tags.is_empty() {
            input["filter"] = json!({ "tags": filter_tags });
        }
        let resp = slots
            .memory_query
            .invoke(Rights::INVOKE, input)
            .map_err(|e| AgentError::MemoryFailed(e.to_string()))?;
        Ok(resp)
    }

    pub fn record(&self, content: Value, tags: Vec<String>) -> Result<Value, AgentError> {
        let slots = AgentSlots::from_bindings(&self.cspace, &self.bindings);
        let text_repr = content.to_string();
        let embed_resp = slots
            .llm_embed
            .invoke(Rights::INVOKE, json!({ "texts": [text_repr] }))
            .map_err(|e| AgentError::LlmFailed(e.to_string()))?;
        let vector: Option<Vec<f32>> = embed_resp
            .get("vectors")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
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
        let resp = slots
            .memory_insert
            .invoke(Rights::INVOKE, input)
            .map_err(|e| AgentError::MemoryFailed(e.to_string()))?;
        Ok(resp)
    }
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

fn build_system_prompt(first_action: bool, allowed_tools: &[String]) -> String {
    let tool_list = if allowed_tools.is_empty() {
        "(no tools available)".to_string()
    } else {
        allowed_tools.join(", ")
    };
    let mut s = String::new();
    s.push_str("You are an agent inside the odyssey kernel. ");
    s.push_str("Available tools: ");
    s.push_str(&tool_list);
    s.push_str(". ");
    if first_action {
        s.push_str("__AGENT_FIRST_ACTION__: this is the first turn; you should call a tool. ");
    } else {
        s.push_str("__AGENT_FINAL_AFTER_TOOL__: the previous tool call returned; you should produce a final answer now. ");
    }
    s.push_str("Reply with JSON: {\"tool_call\":{\"tool\":\"<name>\",\"args\":<json>}} to call a tool, or {\"final\":<value>} to finish. Plain text is treated as a final answer.");
    s
}

fn build_prompt(session: &Session, observation: &Observation) -> String {
    let mut s = String::new();
    s.push_str("Goal: ");
    s.push_str(&session.goal);
    s.push_str("\n\nContext: ");
    s.push_str(&session.context.to_string());
    s.push_str("\n\nHistory (most recent last):\n");
    for step in session.history.iter() {
        match step {
            Step::ToolCall(inv) => {
                s.push_str(&format!("  - tool_call {} {}\n", inv.tool, inv.args))
            }
            Step::ToolResult(r) => {
                let outcome = match &r.outcome {
                    Ok(v) => format!("ok {}", v),
                    Err(e) => format!("err {}", e),
                };
                s.push_str(&format!("  - tool_result {} {}\n", r.tool, outcome));
            }
            _ => {}
        }
    }
    match observation {
        Observation::Tick => s.push_str("\nObservation: (first turn — call a tool)\n"),
        Observation::ToolResult { tool, value, error } => {
            s.push_str(&format!(
                "\nObservation: tool `{tool}` returned {} = {}\n",
                error.as_deref().unwrap_or("ok"),
                value
            ));
        }
        Observation::UserReply(text) => {
            s.push_str(&format!("\nObservation: user reply: {text}\n"));
        }
    }
    s
}

fn build_plan_prompt(tools: &[String]) -> String {
    let tool_list = if tools.is_empty() {
        "(no tools)".to_string()
    } else {
        tools.join(", ")
    };
    format!(
        "You are a planner inside the odyssey kernel. Tools: {tool_list}. \
         Produce a numbered plan (1., 2., 3., ...). Plain text only; \
         do not call any tools."
    )
}

/// Build the `tools` array for native tool calling. Each
/// entry is the tool's name (the cspace name), a short
/// description (from `tool_schema.description` if the
/// builtin published one), and a JSON Schema for the
/// arguments (from `tool_schema.input_schema` or a
/// permissive default). The function silently skips tools
/// that don't carry a `tool_schema` — they remain reachable
/// by name through the cspace, just not advertised to the
/// LLM in the schema-driven way.
fn collect_tool_schemas(cspace: &CapabilitySpace, allowed_tools: &[String]) -> Value {
    let tool_list: Vec<String> = if allowed_tools.is_empty() {
        // No whitelist — advertise every cap that carries a
        // tool_schema. This is the broadest "what can the
        // LLM call" view.
        let names: Vec<String> = cspace.enumerate().into_iter().map(|m| m.name).collect();
        names
    } else {
        allowed_tools.to_vec()
    };
    let mut out: Vec<Value> = Vec::new();
    for name in tool_list {
        let Some(cap) = cspace.lookup_by_name(&name) else {
            continue;
        };
        let meta = cap.meta();
        let schema = match meta.tool_schema.as_ref() {
            Some(s) => s,
            None => continue,
        };
        let description = schema
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let parameters = schema
            .get("input_schema")
            .cloned()
            .unwrap_or(json!({ "type": "object" }));
        out.push(json!({
            "name": name,
            "description": description,
            "parameters": parameters,
        }));
    }
    Value::Array(out)
}

/// Extract the first `tool_call` from a native LLM response
/// (the OpenAI backend returns these as
/// `tool_calls: [{name, arguments}]`). Returns `None` if
/// the response carries no native calls — the caller then
/// falls back to text parsing.
fn first_native_tool_call(llm_resp: &Value) -> Option<ToolInvocation> {
    let tc = llm_resp.get("tool_calls")?.as_array()?.first()?;
    let name = tc.get("name").and_then(Value::as_str)?.to_string();
    let arguments = tc.get("arguments").cloned().unwrap_or(Value::Null);
    Some(ToolInvocation {
        tool: name,
        args: arguments,
    })
}

// ---------------------------------------------------------------------------
// LLM reply parsing — recognises the agent's protocol
// ---------------------------------------------------------------------------

fn parse_llm_reply(
    text: &str,
    history: &mut History,
    allowed_tools: &[String],
    cspace: &CapabilitySpace,
) -> Result<(Step, SessionStatus), AgentError> {
    let trimmed = text.trim();
    if trimmed.starts_with('{')
        && let Ok(v) = serde_json::from_str::<Value>(trimmed)
    {
        if let Some(tc) = v.get("tool_call") {
            let tool = tc
                .get("tool")
                .and_then(Value::as_str)
                .ok_or_else(|| AgentError::LlmFailed("tool_call missing `tool`".into()))?
                .to_string();
            let args = tc.get("args").cloned().unwrap_or(Value::Null);
            if !allowed_tools.is_empty() && !allowed_tools.contains(&tool) {
                return Err(AgentError::ToolDenied {
                    tool,
                    reason: "not in allowed_tools",
                });
            }
            let cap = cspace
                .lookup_by_name(&tool)
                .ok_or_else(|| AgentError::ToolUnknown(tool.clone()))?;
            // Phase M3: see comment in `advance` — the kernel
            // enforces EXECUTE; this redundant plugin-side check
            // is dead. Delete.
            let outcome = cap
                .invoke_dyn_typed(Rights::INVOKE, args.clone())
                .map_err(|e| AgentError::ToolFailed {
                    tool: tool.clone(),
                    error: e.to_string(),
                });
            let tr = ToolResult {
                tool: tool.clone(),
                outcome: outcome.map_err(|e| e.to_string()),
            };
            let invocation = ToolInvocation {
                tool: tool.clone(),
                args,
            };
            history.push(Step::ToolCall(invocation));
            history.push(Step::ToolResult(tr.clone()));
            return Ok((Step::ToolResult(tr), SessionStatus::AwaitingObservation));
        }
        if let Some(final_v) = v.get("final") {
            let f = FinalAnswer {
                value: final_v.clone(),
                reason: FinalReason::Goal,
            };
            history.push(Step::Final(f.clone()));
            return Ok((Step::Final(f), SessionStatus::Done));
        }
    }
    // Plain text — treat as a final answer.
    let f = FinalAnswer {
        value: Value::String(text.to_string()),
        reason: FinalReason::Goal,
    };
    history.push(Step::Final(f.clone()));
    Ok((Step::Final(f), SessionStatus::Done))
}

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
        // Optional `path`: if provided, the session state is
        // serialised to that file *before* the session is
        // removed. The file can later be loaded back via
        // `agent_load`. The path's parent dir is created if
        // it doesn't exist. Without `path`, the cancel is
        // identical to the pre-checkpoint behaviour.
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
    fn open(
        &self,
        input: Value,
    ) -> Result<tokio::sync::mpsc::Receiver<odyssey::core::meta::chunk::CapabilityChunk>, String>
    {
        let id = input
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "agent_stream: expected `session_id` string".to_string())?;
        self.runtime.lookup(id).map_err(|e| e.to_string())?;

        let mut rx_bcast = {
            let sessions = self.runtime.sessions.lock().expect("sessions poisoned");
            let session = sessions
                .get(&SessionId(id.to_string()))
                .ok_or_else(|| format!("agent_stream: session `{id}` not found"))?;
            session.sender.subscribe()
        };

        let (tx, rx) =
            tokio::sync::mpsc::channel::<odyssey::core::meta::chunk::CapabilityChunk>(32);

        let id_owned = id.to_string();
        tokio::spawn(async move {
            loop {
                match rx_bcast.recv().await {
                    Ok(ev) => {
                        let chunk =
                            odyssey::core::meta::chunk::CapabilityChunk::Item(ev.to_value());
                        if tx.send(chunk).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        let chunk = odyssey::core::meta::chunk::CapabilityChunk::Item(
                            json!({ "kind": "lagged", "missed": n, "session_id": id_owned }),
                        );
                        if tx.send(chunk).await.is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        let chunk = odyssey::core::meta::chunk::CapabilityChunk::Done;
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

/// `agent_load` — load a session from a checkpoint file
/// (typically written by `agent_cancel` with a `path`
/// parameter). The new session keeps the *same* id as the
/// original so external bookkeeping survives a save/load
/// round-trip. Returns the new session id.
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

fn parse_limits(v: Option<&Value>) -> SessionLimits {
    let mut l = SessionLimits::default();
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

fn step_to_value(s: &Step) -> Value {
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
// Builtin — one plugin, seven caps
// ---------------------------------------------------------------------------

pub struct AgentRuntimeBuiltin;

impl BuiltinManifest for AgentRuntimeBuiltin {
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("agent_runtime")
            .expose(NAME_START, CONTRACT_START)
            .expose(NAME_RESUME, CONTRACT_RESUME)
            .expose(NAME_CANCEL, CONTRACT_CANCEL)
            .expose(NAME_PLAN, CONTRACT_PLAN)
            .expose_streaming(NAME_STREAM, CONTRACT_STREAM)
            .expose(NAME_RECALL, CONTRACT_RECALL)
            .expose(NAME_RECORD, CONTRACT_RECORD)
            .expose(NAME_LOAD, CONTRACT_LOAD)
            .requires(REACHES[0].0, REACHES[0].1)
            .requires(REACHES[1].0, REACHES[1].1)
            .requires(REACHES[2].0, REACHES[2].1)
            .requires(REACHES[3].0, REACHES[3].1)
            .timeout_ms(30000)
            .build()
    }
}

impl AgentRuntimeBuiltin {
    /// Typed mint — Slice 3 migration: the eight agent_runtime
    /// caps now live in the plugin's own `PluginCspace`; each
    /// arm mints its concrete resource locally, then grants a
    /// derived slot into the orchestrator's global cspace so
    /// the HTTP bridge can still look it up by name. The
    /// returned `SlotId` is the GLOBAL one — the orchestrator's
    /// existing teardown path (`default_ruin` revoking the
    /// returned ids) works unchanged.
    pub fn mint(
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        bindings: &[ResolvedBinding],
    ) -> SlotId {
        let runtime = Arc::new(AgentRuntime::new(
            factory.space().clone(),
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
            other => panic!("agent_runtime: unexpected capability name `{other}`"),
        }
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            AgentRuntimeBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                AgentRuntimeBuiltin::mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }
}

/// Slice 3 helper: mint a single resource into the plugin's
/// own cspace, then grant a derived slot into the
/// orchestrator's global cspace. Generic over `R: Resource`
/// so each agent_runtime cap can hand in its concrete
/// resource type while sharing the cspace dance. The
/// returned `SlotId` is the global one.
fn mint_cap<R>(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    kind: CapKind,
    decl: &CapabilityDecl,
    budget: CapabilityBudget,
    resource: Arc<R>,
) -> SlotId
where
    R: Resource + 'static,
{
    use odyssey::core::rights::rights::{CapabilityRights, Rights};

    let pc = factory.plugin_cspace(plugin);
    let local_slot = pc.mint(kind, decl, budget.clone(), resource);
    let rights = CapabilityRights {
        // agent_runtime is the only builtin that holds REVOKE:
        // session lifecycle (cancel) revokes the session-bus
        // cap so the LLM plugin's invoke_dyn no longer routes
        // deltas to a dead session.
        operations: Rights::INVOKE | Rights::ASSIGN | Rights::REVOKE,
        timeout_ms: budget.timeout_ms(),
    };
    pc.inner()
        .grant_to::<R>(local_slot, factory.space(), rights, decl.name.clone())
        .expect("grant from plugin cspace to global should succeed")
}
