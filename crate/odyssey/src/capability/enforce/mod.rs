//! Capability / enforce — quota runtime state + cspace storage.
//!
//! The "state" side of the kernel. The cspace owns the slot
//! table, derivation parents, and event bus; the quota module
//! owns the runtime accounting (`QuotaState`,
//! `CapabilityBudget`). The pure value types
//! (`QuotaSpec`, `QuotaKind`, `QuotaSnapshot`) live in
//! `core::quota` because they don't need locks or `Clock`.

pub mod quota;
pub mod space;
