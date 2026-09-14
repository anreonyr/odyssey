//! Phase 8: re-export shim.
//!
//! The manifest content physically lives in `crate::core::manifest`.
//! This module re-exports it at the old `host::manifest::Foo` path
//! so the 43+ existing `use crate::host::manifest::*` import sites
//! keep working during the migration.

pub use crate::core::manifest::manifest::{
    from_path, from_toml_str, CapabilityDecl, CapabilityRequirement, HostServiceRef,
    IsolationMode, ManifestBuilder, ManifestError, PluginManifest, ResourceHints,
};
