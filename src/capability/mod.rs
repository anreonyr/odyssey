//! Capability — kernel implementation.
//!
//! Depends only on `crate::core`. The `init::new_kernel` function
//! is the single personality-facing entry point.

pub mod enforce;
pub mod error;
pub mod handle;
pub mod init;

pub use enforce::quota::{CapabilityBudget, QuotaKind, QuotaSnapshot, QuotaSpec, QuotaState};
pub use enforce::space::{
    CapabilitySpace, DeriveKind, CapabilityEvent, GraphEventBus, CapabilityEventReceiver, RevokeMode,
    TryRecvError,
};
pub use error::CapabilityError;
pub use handle::cap::{AnyCapability, Capability};
pub use handle::slot::Slot;
pub use init::new_kernel;
// `Resource` trait moved to core/contract/resource.rs in Phase 8.
