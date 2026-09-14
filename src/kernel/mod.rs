//! Phase 8: re-export shim for the legacy kernel paths.
//!
//! After this commit the kernel layer physically lives under
//! `crate::capability::*`. This shim re-exports the same items
//! at the old `kernel::*` paths so the rest of the codebase
//! (runtime/, plugins/, tests/) can migrate incrementally.

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

// Capability impl types — were inline modules, now re-exports from
// `crate::capability::*`. The modules themselves stay accessible at
// `kernel::cap::*` etc. so existing call sites compile unchanged.
pub mod cap {
    pub use crate::capability::handle::cap::*;
}
pub mod error {
    pub use crate::capability::error::*;
}
pub mod quota {
    pub use crate::capability::enforce::quota::*;
}
// Resource trait moved to core/contract/resource.rs in this commit.
// `kernel::resource` no longer exists; consumers should depend on
// `crate::core::contract::resource::Resource` (or the curated
// `crate::core::Resource` re-export at the crate root).
pub mod slot {
    pub use crate::capability::handle::slot::*;
}
pub mod space {
    pub use crate::capability::enforce::space::*;
}

// Curated re-exports — the public surface of the kernel.
pub use cap::{AnyCapability, Capability};
pub use chunk::CapabilityChunk;
pub use clock::{Clock, MockClock, SystemClock};
pub use error::CapabilityError;
pub use ids::{CapabilityId, PluginId, SlotId};
pub use kind::CapKind;
pub use meta::{AuthorityContract, CapabilityAction, CapabilityMeta, Protocol};
pub use quota::{CapabilityBudget, QuotaKind, QuotaSnapshot, QuotaSpec, QuotaState};
pub use rights::{parse_operation, CapabilityRights, OperationRights};
pub use slot::Slot;
pub use space::{
    CapabilitySpace, DeriveKind, GraphEvent, GraphEventBus, GraphEventReceiver,
    TryRecvError,
};
// CapabilityGraph was deleted in the Phase 8 cleanup (zero
// production callers; only tests used it). The legacy re-export
// is removed to force callers to drop their dependency.
