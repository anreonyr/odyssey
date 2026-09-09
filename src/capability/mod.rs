//! Capability model — `CapabilitySpace → Slot<R> → Capability<R>`.
//!
//! ## Hierarchy
//!
//! - `CapabilitySpace` — the namespace (seL4 CSpace)
//! - `Slot<R>` — typed, unforgeable reference to a slot (the unit of possession)
//! - `Capability<R>` — what occupies a slot
//!
//! ## Submodules
//!
//! - `types` — `CapKind`, ids, metadata, rights, budget, chunks, error
//! - `resource` — the `Resource` trait (both `invoke` and `open`, defaults
//!   return `Err`)
//! - `cap` — `Capability<R>` and the erased `AnyCapability` view
//! - `cspace` — `CapabilitySpace`, `SlotId`, `Slot<R>`, and the four
//!   operations (grant / transfer / restrict / revoke)
//!
//! ## Sync vs stream distinction
//!
//! The single `Resource` trait provides both `invoke` and `open` methods.
//! Default impls return `Err` with a clear "not a sync/streaming
//! capability" message; concrete resources override the one that
//! applies. The capability carries a runtime `CapKind` so the wrong
//! method call is caught early with a typed error message instead of
//! falling through to a generic "not implemented" panic.
//!
//! seL4 has type-level kinds via different kernel object types; here
//! we trade compile-time distinction for a uniform public API.
//!
//! ## Possession
//!
//! Plugins hold `Slot<R>` (unforgeable). The CSpace can revoke slot
//! contents; the slot reference itself stays valid but lookups fail.

pub mod cap;
pub mod cspace;
pub mod resource;
pub mod types;

pub use cap::{AnyCapability, Capability};
pub use cspace::{CapabilitySpace, Slot};
pub use resource::Resource;
pub use types::{
    CapabilityBudget, CapabilityChunk, CapabilityError, CapabilityId, CapabilityMeta,
    CapabilityRights, CapKind, SlotId,
};
#[allow(unused_imports)]
use CapabilityError as _;

// Bring `Any` into scope for the helper.
use std::any::Any;

use crate::host::manifest::{CapabilityDecl, PluginId};

/// Construct a `CapabilityMeta` from a manifest declaration + budget.
/// Used by the factory at mint time.
pub fn meta_from_decl(
    id: CapabilityId,
    decl: &CapabilityDecl,
    plugin: &PluginId,
    budget: &CapabilityBudget,
) -> CapabilityMeta {
    CapabilityMeta {
        id,
        name: decl.name.clone(),
        plugin: plugin.clone(),
        in_type: decl.in_type.clone(),
        out_type: decl.out_type.clone(),
        streaming: decl.streaming,
        timeout_ms: budget.timeout_ms,
    }
}

// `Any` is in scope so the trait-bound on `AnyCapability: Any` resolves
// correctly in this module's re-exports.
#[allow(unused_imports)]
use Any as _;