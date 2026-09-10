//! Quota subsystem — `QuotaSpec` (declaration) + `QuotaState` (accounting)
//! + `CapabilityBudget` (per-call ceiling + wall-clock counter).
//!
//! Phase 5 split from the old `capability::types`. The split mirrors
//! the council memo §6 — `quota/` is one of the seven sub-concepts
//! under `kernel/` worth its own directory.

pub mod spec;
pub mod state;

pub use spec::{QuotaKind, QuotaSnapshot, QuotaSpec};
pub use state::{CapabilityBudget, QuotaState};
