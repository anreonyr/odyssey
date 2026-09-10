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
//!
//! ## Phase 6 file split
//!
//! The agent's `handler.rs` was 712 LOC and bundled four
//! concerns: the `AgentResource` struct + constructors, the
//! sync dispatch path, the streaming interpreter, and the
//! cordis plugin glue. Phase 6 split it into five focused
//! files in this module:
//!
//! - `handler.rs`  — struct, constructors, accessors, the
//!   shared `parse_operation` helper.
//! - `dispatch.rs` — `impl Resource for AgentResource { fn
//!   invoke }`. Sync dispatch.
//! - `stream.rs`   — `impl Resource for AgentResource { fn open
//!   }`, the `run_program` interpreter, `RunStats`, and
//!   event-emission helpers.
//! - `plugin.rs`   — cordis glue: `handler`, `handler_from_plan`,
//!   `agent_plugin`.
//! - `panic.rs`    — `panic_payload_to_str` formatter used by
//!   the streaming path's panic-catch.

mod dispatch;
mod handler;
mod panic;
mod plugin;
mod stream;

pub mod program;
pub use handler::AgentResource;
pub use plugin::{agent_plugin, handler as agent_handler, handler_from_plan};
pub use program::ProgramStep;

mod manifest;
pub use manifest::manifest;
