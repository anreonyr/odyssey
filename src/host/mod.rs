//! Host layer — composition of manifests into a populated capability
//! kernel.
//!
//! Phase 8: the modules previously owned by `host/` are moving
//! out to `core/` (manifest types), `personality/composition/`
//! (resolver), and `personality/lifecycle/` (factory + mint).
//! What's left in `host/` is re-export shims pointing at the new
//! homes. The directory will be deleted entirely once all
//! callers migrate off the `host::*` paths.
//!
//! Phase 8 cleanup: the `pipeline` module (linear sync-cap
//! composition) was deleted — it had zero production callers
//! after the Phase 4 agent-driven composition pattern landed.

pub mod factory;
pub mod manifest;
pub mod mint;
pub mod resolver;

// Curated re-exports.
pub use factory::CapabilityFactory;
pub use manifest::{
    CapabilityDecl, CapabilityRequirement, HostServiceRef, IsolationMode,
    ManifestBuilder, ManifestError, PluginManifest, ResourceHints,
};
pub use crate::kernel::ids::PluginId;
pub use mint::{meta_from_decl, namespace_for};
pub use resolver::{resolve, ResolveError, Reachable, ResolvedBinding, ResolvedPlan};
