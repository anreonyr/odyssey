//! Kernel — the pure capability core.
//!
//! Phase 5: this module is the **pure kernel**. It contains no I/O,
//! no `Instant::now()` direct calls (those go through `clock::Clock`),
//! no filesystem, no tokio runtime (except where `tokio::sync::mpsc`
//! is the canonical stream-channel type — that migration is deferred).
//!
//! Submodules:
//!
//! Phase 8 split: the leaf value-type modules (`ids`, `kind`,
//! `clock`, `chunk`, `rights`) have been moved to `crate::core::*`.
//! The kernel module is now a thin re-export shim for those
//! modules and a real implementation site for the rest. This
//! keeps existing `use crate::kernel::ids::CapabilityId` import
//! paths working during the multi-commit migration.
//!
//! Submodules:
//!
//! - `ids`, `kind`, `clock`, `chunk`, `rights` — re-export shims
//!   pointing at the moved files under `crate::core::*`.
//! - `meta` — `CapabilityMeta` + `AuthorityContract` + `Protocol`.
//! - `error` — `CapabilityError` (typed variants).
//! - `resource` — `Resource` trait.
//! - `quota` — `QuotaSpec` + `QuotaState` + `CapabilityBudget`.
//! - `cap` — `Capability<R>` (typed) + `AnyCapability` (erased).
//! - `slot` — `Slot<R>` (unforgeable typed reference).
//! - `space` — `CapabilitySpace` (the namespace) + events + graph + namespace helpers.
//!
//! ## Phase 5 dependencies
//!
//! - `kernel` → nothing (it is a leaf in the dependency graph; host
//!   and runtime depend on it, not the other way around).
//! - `host` → depends on `kernel`.
//! - `runtime` → depends on `host` and `kernel`.
//!
//! ## Phase 5 fixes included in this module
//!
//! D2, M1, D3, D1, M2, M3, M4, n1, n3, R1, R4, R5, R6, R7, m3.
//! See `CHANGELOG.md` Phase 5 entry for the full list.

// Phase 8: leaf value-type modules physically live under
// `crate::core::*`. Inline `pub mod` blocks here re-export
// from the canonical core path so the old `use crate::kernel::ids::*`
// paths continue to work — and both paths point at the SAME
// items (no duplicate type identity).
pub mod ids {
    pub use crate::core::identity::ids::*;
}
pub mod kind {
    pub use crate::core::identity::kind::*;
}
pub mod clock {
    pub use crate::core::clock::clock::*;
}
pub mod chunk {
    pub use crate::core::meta::chunk::*;
}
pub mod meta {
    pub use crate::core::meta::meta::*;
}
pub mod rights {
    pub use crate::core::rights::rights::*;
}

pub mod cap;
pub mod error;
pub mod quota;
pub mod resource;
pub mod slot;
pub mod space;

// Curated re-exports — the public surface of the kernel.
pub use cap::{AnyCapability, Capability};
pub use chunk::CapabilityChunk;
pub use clock::{Clock, MockClock, SystemClock};
pub use error::CapabilityError;
pub use ids::{CapabilityId, PluginId, SlotId};
pub use kind::CapKind;
pub use meta::{AuthorityContract, CapabilityAction, CapabilityMeta, Protocol};
pub use quota::{CapabilityBudget, QuotaKind, QuotaSnapshot, QuotaSpec, QuotaState};
pub use resource::Resource;
pub use rights::{parse_operation, CapabilityRights, OperationRights};
pub use slot::Slot;
pub use space::{
    CapabilityGraph, CapabilitySpace, DeriveKind, GraphEvent, GraphEventBus, GraphEventReceiver,
    TryRecvError,
};

