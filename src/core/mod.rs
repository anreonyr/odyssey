//! Core — shared value types + abstract traits.
//!
//! Pure leaf layer. No I/O, no `Instant::now()` direct calls
//! (use `clock::Clock`), no filesystem, no cordis, no
//! capability kernel implementation. Both `capability` and
//! `personality` depend on `core`; `core` depends on nothing
//! internal to the crate.
//!
//! Submodules:
//!
//! - `identity` — `CapabilityId`, `SlotId`, `PluginId`, `CapKind`.
//! - `clock` — `Clock` trait + `SystemClock` + `MockClock`.
//! - `meta` — `CapabilityMeta`, `AuthorityContract`, `Protocol`,
//!   `CapabilityChunk`.
//! - `rights` — `OperationRights`, `CapabilityRights`,
//!   `parse_operation`.

pub mod clock;
pub mod identity;
pub mod meta;
pub mod rights;

// Curated re-exports — the public surface of core.
// Each submodule is itself a folder; the inner file shares the
// folder name. So `core::clock::clock::Clock` etc. — verbose but
// follows the strict-hierarchy rule.
pub use clock::clock::{Clock, MockClock, SystemClock};
pub use identity::ids::{CapabilityId, PluginId, SlotId};
pub use identity::kind::CapKind;
pub use meta::meta::{AuthorityContract, CapabilityAction, CapabilityMeta, Protocol};
pub use meta::chunk::CapabilityChunk;
pub use rights::rights::{parse_operation, CapabilityRights, OperationRights};
