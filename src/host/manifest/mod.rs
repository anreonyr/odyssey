//! Host-side manifest data shape.
//!
//! Phase 5: split from `kernel::manifest`. The data types live
//! here (parsing is the host's concern); `PluginId` itself
//! migrates to `kernel::ids` because it is a pure identity used
//! everywhere.

pub mod builder;
pub mod types;

pub use builder::ManifestBuilder;
pub use types::{
    CapabilityDecl, CapabilityRequirement, DependencyRef, HostServiceRef, IsolationMode,
    ManifestError, PluginManifest, ResourceHints,
};
