//! Manifest loader — explicit enumeration of every runtime
//! plugin's manifest function.
//!
//! Adding a new runtime plugin requires editing this list; the
//! compile-time `tests` below assert the loaded list is sorted by
//! plugin name and contains no duplicates.
//!
//! Test-only plugins (under `test_only/*`) are reached directly via
//! `factory.mint` in their test crate, not through boot, so they
//! are absent from this enumeration.

use crate::host::manifest::PluginManifest;

/// Collect every runtime plugin's manifest, sort by plugin name.
///
/// Returns `Box<dyn Error>` so the caller (`run` in `mod.rs`) can
/// surface a uniform boot error type.
pub fn load_manifests() -> Result<Vec<PluginManifest>, Box<dyn std::error::Error>> {
    use crate::plugins::{
        agent::manifest as agent_manifest,
        database::manifest as database_manifest,
        echo::{
            basic::manifest as echo_basic_manifest,
            chain::manifest as echo_chain_manifest,
            stream::manifest as echo_stream_manifest,
        },
        embedder::manifest as embedder_manifest,
        generator::manifest as generator_manifest,
        http::manifest as http_manifest,
        reverse::manifest as reverse_manifest,
        sandbox::manifest as sandbox_manifest,
        slow::manifest as slow_manifest,
    };

    let manifests = [
        database_manifest(),
        embedder_manifest(),
        echo_basic_manifest(),
        echo_chain_manifest(),
        echo_stream_manifest(),
        agent_manifest(),
        generator_manifest(),
        http_manifest(),
        reverse_manifest(),
        sandbox_manifest(),
        slow_manifest(),
    ];
    let mut out: Vec<PluginManifest> = manifests.iter().map(|m| (*m).clone()).collect();
    out.sort_by(|a, b| a.plugin.name.cmp(&b.plugin.name));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_manifests_returns_sorted_unique() {
        let manifests = load_manifests().expect("load_manifests");
        let names: Vec<&str> = manifests.iter().map(|m| m.plugin.name.as_str()).collect();

        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(
            names, sorted,
            "manifests must be sorted by plugin name (Phase 1 contract)"
        );

        let mut unique = names.clone();
        unique.dedup();
        assert_eq!(
            names.len(),
            unique.len(),
            "no duplicate plugin names allowed (Phase 1 contract)"
        );
    }

    #[test]
    fn load_manifests_returns_eleven() {
        // 11 runtime plugins are wired into boot. Bumping this
        // count means adding a new runtime plugin; both this
        // assertion and `load_manifests` must move together.
        let manifests = load_manifests().expect("load_manifests");
        assert_eq!(manifests.len(), 11, "expected 11 runtime plugins");
    }
}
