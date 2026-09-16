//! Core — shared value types + abstract traits.
//!
//! Pure leaf layer. No I/O, no `Instant::now()` direct calls
//! (use `clock::Clock`), no filesystem, no
//! capability kernel implementation. Both `capability` and
//! `personality` depend on `core`; `core` depends on nothing
//! internal to the crate.
//!
//! Submodules:
//!
//! - `identity` — `CapabilityId`, `SlotId`, `PluginId`, `CapKind`.
//! - `clock` — `Clock` trait + `SystemClock` + `MockClock`.
//! - `meta` — `CapabilityMeta`, `CapabilityChunk`.
//! - `rights` — `OperationRights`, `CapabilityRights`,
//!   `parse_operation`.
//! - `manifest` — `PluginManifest`, `ManifestBuilder`,
//!   `CapabilityDecl`, `CapabilityRequirement`, `HostServiceRef`,
//!   `IsolationMode`, `ResourceHints`.
//! - `contract` — abstract traits: `Resource`, `BuiltinManifest`.

pub mod clock;
pub mod contract;
pub mod identity;
pub mod manifest;
pub mod meta;
pub mod quota;
pub mod rights;

// Curated re-exports — the public surface of core.
// Each submodule is itself a folder; the inner file shares the
// folder name. So `core::clock::clock::Clock` etc. — verbose but
// follows the strict-hierarchy rule.
pub use clock::clock::{Clock, MockClock, SystemClock};
pub use contract::builtin::BuiltinManifest;
pub use contract::resource::Resource;
pub use identity::ids::{CapabilityId, PluginId, SlotId};
pub use identity::kind::CapKind;
pub use manifest::manifest::{
    CapabilityDecl, CapabilityRequirement, HostServiceRef, IsolationMode, ManifestBuilder,
    PluginManifest, ResourceHints,
};
pub use meta::chunk::CapabilityChunk;
pub use meta::meta::CapabilityMeta;
pub use quota::quota::{QuotaKind, QuotaSnapshot, QuotaSpec};
pub use rights::rights::{CapabilityRights, OperationRights, parse_operation};
