//! Kernel — the pure capability core.
//!
//! Phase 5: this module is the **pure kernel**. It contains no I/O,
//! no `Instant::now()` direct calls (those go through `clock::Clock`),
//! no filesystem, no tokio runtime (except where `tokio::sync::mpsc`
//! is the canonical stream-channel type — that migration is deferred).
//!
//! Submodules:
//!
//! - `ids` — `CapabilityId`, `SlotId`, `PluginId`. Pure value types.
//! - `kind` — `CapKind` (sync vs stream).
//! - `rights` — `OperationRights` + `CapabilityRights` + `parse_operation`.
//! - `meta` — `CapabilityMeta` + `AuthorityContract` + `Protocol`.
//! - `chunk` — `CapabilityChunk` (stream item enum).
//! - `clock` — `Clock` trait + `SystemClock` + `MockClock`.
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

pub mod cap;
pub mod chunk;
pub mod clock;
pub mod error;
pub mod ids;
pub mod kind;
pub mod meta;
pub mod quota;
pub mod resource;
pub mod rights;
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
