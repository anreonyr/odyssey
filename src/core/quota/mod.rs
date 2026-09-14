//! Core quota value types — `QuotaSpec`, `QuotaKind`,
//! `QuotaSnapshot`. Pure data; the runtime-state
//! counterparts (`QuotaState`, `CapabilityBudget`) live in
//! `capability::enforce::quota`.

pub mod quota;

pub use quota::{QuotaKind, QuotaSnapshot, QuotaSpec};
