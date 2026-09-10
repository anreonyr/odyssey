//! `Reachable` — one row of a consumer's binding table.
//!
//! Phase 4 P4.1 promoted `Reachable` from `test_only/agent/` to
//! the capability module because Generator's `HttpModel` (and
//! every future plugin that consumes another plugin's
//! capability) needs the same shape.
//!
//! `handle` is the local name the consumer uses for dispatch
//! (the input's `target` field, or the model's lookup key);
//! `capability` is the cspace name `lookup_by_name` resolves
//! against.
//!
//! ## Why this lives in `capability/`, not `agent/`
//!
//! `Reachable` is a thin view over `ResolvedBinding` (a
//! `kernel` type). It is consumed by:
//!
//! - [`crate::plugins::test_only::agent::AgentResource`]
//!   (test-only; the agent test surface)
//! - [`crate::plugins::generator::model::HttpModel`] (Phase 4
//!   P4.1; generator consuming the http capability)
//!
//! Both are consumers; neither owns the data. The view belongs
//! in `capability/` next to the binding semantics.

use crate::kernel::resolver::ResolvedBinding;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reachable {
    pub handle: String,
    pub capability: String,
}

impl Reachable {
    pub fn from_binding(b: &ResolvedBinding) -> Self {
        Self {
            handle: b.handle.clone(),
            capability: b.capability.clone(),
        }
    }

    pub fn new(handle: impl Into<String>, capability: impl Into<String>) -> Self {
        Self {
            handle: handle.into(),
            capability: capability.into(),
        }
    }
}
