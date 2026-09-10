//! Capability-Native Agent plugin — Phase 4 P4.4.
//!
//! The agent is a generic program interpreter. The caller
//! submits a `Vec<ProgramStep>` at invoke time; the agent
//! iterates the program, dispatches each step through the
//! reachable table, and emits a streaming trace of outcomes.
//!
//! ## Why streaming
//!
//! Each step is an event (start / ok / skip / deny / fail).
//! Streaming makes the trace observable in real time — useful
//! for the P4.5 "same program, different env" proof, where
//! the operator can watch two agent runs diverge.
//!
//! ## Why runtime, not test_only
//!
//! The agent moved from `test_only/agent` to here in Phase 4
//! P4.4. It's a plugin like any other: its capabilities come
//! from the binding table; the boot pipeline mints it
//! normally. The test surface still uses it through the same
//! module path.

pub mod handler;
pub use handler::{agent_plugin, handler as agent_handler, handler_from_plan, AgentResource};

pub mod program;
pub use program::ProgramStep;

mod manifest;
pub use manifest::manifest;
