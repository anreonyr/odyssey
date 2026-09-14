//! Personality / lifecycle / boot — manifest loader + print.
//!
//! Phase 8: moved from `runtime/lifecycle/{manifests,print}.rs`.
//! The Phase 5 enumeration of 11 runtime plugin manifests
//! (`crate::plugins::agent::manifest`, etc.) referenced the
//! `plugins/*` modules which are being deleted in a later commit.
//!
//! In the new layout, the actual manifest enumeration is provided
//! by the `builtins/` workspace member, which the example
//! binary feeds into `mint_all`. The library itself does not
//! enumerate builtins — it just provides the orchestration API
//! (`load_manifests`, `print_manifests`, `run`) that the binary
//! drives.
//!
//! For now `load_manifests` returns an empty list. This is a
//! transitional state; commit "create builtins/ workspace
//! member" wires the real enumeration back in.

use crate::core::manifest::manifest::PluginManifest;

/// Load manifests. Phase 8 transitional: returns an empty
/// list. The example binary constructs manifests via
/// `builtins::*::manifest()` directly and passes them into
/// `mint_all` rather than via this loader.
pub fn load_manifests() -> Result<Vec<PluginManifest>, Box<dyn std::error::Error>> {
    Ok(Vec::new())
}

/// Print manifests as a boot log table. Phase 8 transitional:
/// no-op when the manifest list is empty.
pub fn print_manifests(_manifests: &[PluginManifest]) {}
