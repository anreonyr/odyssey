//! Agent module — combines the observer half (`core`) and the
//! runtime half (`runtime` + `builtin`). After the refactor,
//! this is the only entry point for the `agent` plugin; the
//! old `agent_runtime` plugin and the `memory` plugin are
//! folded into here.
//!
//! Public surface (re-exported at the module root so callers
//! keep a single import path):
//! - Observer: `AgentCore`, `AgentError` (observer's), `Reach`,
//!   `AgentListBuiltin`, `AgentDescribeBuiltin`,
//!   `AgentListResource`, `AgentDescribeResource`.
//! - Runtime: `AgentRuntime`, `AgentRuntimeBuiltin`,
//!   `AgentStartResource`, `AgentResumeResource`, ...
//! - Memory: `MemoryBackend`, `InMemoryBackend`,
//!   `FileMemoryBackend` (internal but exposed for tests).
//! - Session types: `SessionId`, `Step`, `AgentEvent`, ...

mod builtin;
mod core;
mod memory;
mod prompt;
mod runtime;
mod session;
mod types;

// Observer re-exports.
pub use core::{
    AgentCore, AgentDescribeBuiltin, AgentDescribeResource, AgentError, AgentListBuiltin,
    AgentListResource, CONTRACT_DESCRIBE, CONTRACT_LIST, Reach,
};

// Runtime re-exports.
pub use builtin::{
    AgentCancelResource, AgentLoadResource, AgentMemoryRecallResource, AgentMemoryRecordResource,
    AgentPlanResource, AgentResumeResource, AgentRuntimeBuiltin, AgentStartResource,
    AgentStreamResource, step_to_value,
};
pub use runtime::AgentRuntime;

// Memory re-exports.
pub use memory::{
    FileMemoryBackend, InMemoryBackend, MemoryBackend, Record, insert_record, pick_backend,
    query_records,
};

// Session helpers.
pub use session::{
    SessionEventBusResource, global_sessions, mint_session_event_bus, subscribe_session_broadcast,
};

// Type re-exports (data shapes).
pub use types::{
    AgentError as RuntimeAgentError, AgentEvent, FinalAnswer, FinalReason, History, LlmReply,
    Observation, Session, SessionCheckpoint, SessionId, SessionLimits, SessionStatus, Step,
    ToolInvocation, ToolResult,
};
