//! Capability — kernel implementation.
//!
//! Depends only on `crate::core`. Personality calls into the
//! kernel via `CapabilityFactory::new(cspace)` (in
//! `personality::lifecycle::mint`), constructs a `CapabilitySpace`
//! directly via `CapabilitySpace::new()`, and operates on the
//! space via lookup / enumerate / revoke.

pub mod enforce;
pub mod error;
pub mod handle;

pub use enforce::quota::{CapabilityBudget, QuotaState};
// `QuotaSpec`, `QuotaKind`, `QuotaSnapshot` live in `core::quota`
// (re-exported from the crate root as `crate::QuotaSpec` etc.).
pub use enforce::space::{
    CapabilitySpace, DeriveKind, CapabilityEvent, GraphEventBus, CapabilityEventReceiver, RevokeMode,
    TryRecvError,
};
pub use error::CapabilityError;
pub use handle::cap::{AnyCapability, Capability};
pub use handle::slot::Slot;
// `Resource` trait moved to core/contract/resource.rs in Phase 8.
