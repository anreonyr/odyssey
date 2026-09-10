//! `Reachable` — one row of a consumer's binding table.
//!
//! Phase 5: split from `capability::reachable`. It is a thin
//! view over `ResolvedBinding`, used by every consumer that
//! needs to dispatch by capability name (`Generator`,
//! `Agent`, etc.).
//!
//! `handle` is the local name the consumer uses for dispatch
//! (the input's `target` field, or the model's lookup key);
//! `capability` is the cspace name `lookup_by_name` resolves
//! against.

use crate::host::resolver::ResolvedBinding;

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
