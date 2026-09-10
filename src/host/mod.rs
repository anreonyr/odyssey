//! Host layer — composition of manifests into a populated capability
//! kernel.
//!
//! Phase 5: split from `kernel::*`. The host layer owns:
//!
//! - `manifest` — TOML parsing + validation + `ManifestBuilder`.
//! - `factory` — `CapabilityFactory` mints typed `Capability<R>`.
//! - `mint` — `meta_from_decl` + `namespace_for` helpers.
//! - `resolver` — capability-keyed dependency resolution.
//! - `pipeline` — composes sync caps into stage lists.
//!
//! Depends on `crate::kernel` (the pure kernel). Does not touch
//! the runtime layer directly.

pub mod factory;
pub mod manifest;
pub mod mint;
pub mod pipeline;
pub mod resolver;

// Back-compat: the Phase 4 `host::Reachable` path stays
// compiling until every plugin has migrated to
// `host::resolver::Reachable`.
pub mod reachable {
    pub use crate::host::resolver::plan::Reachable;
}

// Curated re-exports.
pub use factory::CapabilityFactory;
pub use manifest::{
    CapabilityDecl, CapabilityRequirement, DependencyRef, HostServiceRef, IsolationMode,
    ManifestBuilder, ManifestError, PluginManifest, ResourceHints,
};
pub use crate::kernel::ids::PluginId;
pub use mint::{meta_from_decl, namespace_for};
pub use pipeline::{Pipeline, PipelineError, SyncStage};
pub use resolver::{resolve, ResolveError, Reachable, ResolvedBinding, ResolvedPlan};
