//! Core quota value types — `QuotaSpec`, `QuotaKind`,
//! `QuotaSnapshot`. Pure data; the runtime-state
//! counterparts (`QuotaState`, `CapabilityBudget`) live in
//! `capability::enforce::quota`.

// See `core/clock/mod.rs` for the rationale.
#[allow(clippy::module_inception)]
pub mod quota;

pub use quota::{QuotaKind, QuotaSnapshot, QuotaSpec};
