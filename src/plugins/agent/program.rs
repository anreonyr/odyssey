//! `ProgramStep` — one operation the agent performs.
//!
//! Phase 4 P4.4: the agent is a **generic program interpreter**.
//! The caller submits a `Vec<ProgramStep>` at invoke time; the
//! agent iterates the program, dispatches each step through the
//! reachable table, and emits a stream of events describing
//! what happened.
//!
//! A step has four outcomes:
//!
//! | outcome      | when                                                 |
//! | ------------ | ---------------------------------------------------- |
//! | `ok`         | handle in reachable + slot in cspace + invoke ok     |
//! | `skip`       | handle not in reachable (env doesn't grant)          |
//! | `deny`       | reachable but cap's authority doesn't include `op`   |
//! | `fail`       | invoke itself returned an Err                        |
//!
//! **Crucially**, the agent does not panic on skip/deny/fail.
//! It records the outcome and proceeds to the next step.
//! This is the Phase 4 thesis property: capability failure is
//! data, not crash.
//!
//! ## Fields
//!
//! - `handle` — local name the agent matches against the
//!   reachable table's `handle` field.
//! - `op` — optional authority action. If `Some`, the agent
//!   checks `meta.authority.operation_for(op)` before invoking.
//!   `None` means "trust the caller" (still goes through the
//!   reachable check).
//! - `input` — JSON passed verbatim to the underlying
//!   `Resource::invoke` or `Resource::open` depending on
//!   whether the capability is sync or streaming.
//! - `save_as` — optional key. The step's output is stored
//!   under this key for later steps to reference. Phase 4
//!   does not yet implement substitution; the field is
//!   reserved for P4.7 (composition across agents).

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProgramStep {
    pub handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub op: Option<String>,
    #[serde(default)]
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save_as: Option<String>,
}

impl ProgramStep {
    pub fn call(handle: impl Into<String>, input: Value) -> Self {
        Self {
            handle: handle.into(),
            op: None,
            input,
            save_as: None,
        }
    }

    pub fn with_op(mut self, op: impl Into<String>) -> Self {
        self.op = Some(op.into());
        self
    }

    pub fn with_save_as(mut self, save_as: impl Into<String>) -> Self {
        self.save_as = Some(save_as.into());
        self
    }
}
