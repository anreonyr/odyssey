//! Capability / enforce — quota accounting + cspace storage.
//!
//! The "state" side of the kernel. The cspace owns the slot
//! table, derivation parents, and event bus; the quota module
//! owns rate-limit + wall-clock budgets.

pub mod quota;
pub mod space;
