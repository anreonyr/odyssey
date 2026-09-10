//! Odyssey library — the capability kernel + plugins.
//!
//! Three layers (Phase 5):
//!
//! - `kernel` — the pure kernel: capability data types, slot
//!   algebra, quota accounting, namespace tree, graph events.
//!   No I/O, no `Instant::now()` direct calls (use `clock::Clock`),
//!   no filesystem, no cordis.
//! - `host` — composition layer: parses manifests, resolves
//!   dependencies, mints typed caps into the cspace. Depends on
//!   `kernel`.
//! - `runtime` — adapter layer: lifecycle, HTTP bridge, plugin
//!   mint dispatch. Depends on `host` and `kernel`.
//! - `plugins` — plugin bodies. Depend on `kernel` (for the
//!   `Capability<R>` / `Resource` types they implement) and
//!   `host` (for `PluginManifest` / `CapabilityFactory`).
//!
//! Back-compat re-exports:
//!
//! - `capability` — flat re-export of the Phase 4 capability
//!   surface (`Capability`, `CapabilitySpace`, `Slot`, etc.).
//!   Used by integration tests that predate Phase 5's kernel/host
//!   split. Sub-modules `cspace`, `events`, `graph` mirror the
//!   Phase 4 sub-module names.

pub mod boot;
pub mod host;
pub mod kernel;
pub mod plugins;

pub mod capability {
    //! Phase 4 `capability` namespace re-expressed on top of the
    //! Phase 5 `kernel` split. Every type lives in `kernel` (or
    //! `host`, for composition-only types like `Reachable`); this
    //! module just glues them back together so existing call sites
    //! (`odyssey::capability::CapabilitySpace`, `..::Slot`, ..)
    //! keep compiling.

    pub use crate::kernel::cap::{AnyCapability, Capability};
    pub use crate::kernel::chunk::CapabilityChunk;
    pub use crate::kernel::error::CapabilityError;
    pub use crate::kernel::ids::{CapabilityId, PluginId, SlotId};
    pub use crate::kernel::kind::CapKind;
    pub use crate::kernel::meta::{AuthorityContract, CapabilityMeta, Protocol};
    pub use crate::kernel::quota::{
        CapabilityBudget, QuotaKind, QuotaSnapshot, QuotaSpec, QuotaState,
    };
    pub use crate::kernel::resource::Resource;
    pub use crate::kernel::rights::{parse_operation, CapabilityRights, OperationRights};
    pub use crate::kernel::slot::Slot;
    pub use crate::kernel::space::{
        CapabilityGraph, CapabilitySpace, DeriveKind, GraphEvent, GraphEventBus,
        GraphEventReceiver, TryRecvError,
    };

    // Host-level type that the Phase 4 surface exposed under
    // `capability::Reachable`. Lives in `host` in Phase 5; the
    // re-export keeps the old path compiling.
    pub use crate::host::Reachable;

    /// Phase 4 sub-module alias for `kernel::space`. Tests that
    /// `use odyssey::capability::cspace::CapabilitySpace;` still
    /// resolve.
    pub mod cspace {
        pub use crate::kernel::space::{CapabilityGraph, CapabilitySpace};
    }

    /// Phase 4 sub-module alias for `kernel::space::events`.
    pub mod events {
        pub use crate::kernel::space::events::{
            DeriveKind, GraphEvent, GraphEventBus, GraphEventReceiver, TryRecvError,
        };
    }

    /// Phase 4 sub-module alias for `kernel::space::graph`.
    pub mod graph {
        pub use crate::kernel::space::graph::{CapabilityGraph, CapabilityNode, GraphNode, NamespaceNode};
    }
}
