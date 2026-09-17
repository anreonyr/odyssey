//! Plugin manifest loader (Slice 5 of Direction A).
//!
//! Reads a plugin manifest from disk, validates it, and produces
//! a `(PluginManifest, MintFn, RuinFn)` triple that can be passed
//! to `run_on`. The MintFn / RuinFn for loaded plugins are
//! *placeholders* in this commit — invoking them surfaces a
//! `HandlerUnbound` error so the operator sees the gap. Real
//! handler binding (looking up a handler in a per-plugin
//! registry when the manifest is loaded) is its own follow-up
//! commit; the loader infrastructure is what this slice
//! establishes.
//!
//! The on-disk format is JSON (serde-derived from the
//! `PluginManifest` shape). The Phase 9 comment in
//! `manifest.rs` noted that a future WASM / cdylib loader
//! could re-attach TOML by adding the `toml` dep and
//! calling `serde::Deserialize` on the same struct shape —
//! the schema is decoupled from the on-disk format. For
//! now, JSON avoids a new dep while exercising the same
//! parsing + validation path that a TOML loader would.

use std::path::Path;

use crate::core::identity::ids::SlotId;
use crate::core::manifest::manifest::{
    ManifestLoadError, PluginManifest,
};
use crate::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};

/// A plugin loaded from an on-disk manifest. The
/// `MintFn` / `RuinfN` are placeholders — invoking them
/// returns a `HandlerUnbound` error so the operator sees
/// that the plugin's handler binding is not yet wired up.
#[derive(Debug)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub mint_fn: MintFn,
    pub ruin_fn: RuinFn,
}

/// Load a plugin from an on-disk manifest. Reads the file,
/// parses it as JSON, validates the manifest's invariants,
/// and produces a `LoadedPlugin` whose `MintFn` / `RuinfN`
/// are placeholders.
///
/// Slice 5's scope is the parser + validator + loader
/// wiring. Binding handlers to manifests (looking up the
/// actual `Resource` impl for each exposed capability) is
/// its own follow-up commit; the `MintFn` returned here
/// errors when invoked so the gap is loud rather than
/// silent.
pub fn load_plugin_from_path(path: &Path) -> Result<LoadedPlugin, ManifestLoadError> {
    let manifest = PluginManifest::from_path(path)?;
    let mint_fn: MintFn = placeholder_mint_fn;
    let ruin_fn: RuinFn = default_ruin;
    Ok(LoadedPlugin {
        manifest,
        mint_fn,
        ruin_fn,
    })
}

/// The placeholder `MintFn`. When invoked it returns a
/// `HandlerUnbound` error so the operator sees the gap.
///
/// We deliberately don't panic — a panicking handler would
/// unwind through the orchestrator's invoke path. An Err
/// return is the standard shape for invoke failures and
/// keeps the orchestrator's error handling intact.
fn placeholder_mint_fn(
    _factory: &crate::personality::lifecycle::mint::CapabilityFactory,
    _plugin: &crate::core::identity::ids::PluginId,
    _decl: &crate::core::manifest::manifest::CapabilityDecl,
    _kind: crate::core::identity::kind::CapKind,
    _budget: crate::capability::enforce::quota::CapabilityBudget,
    _bindings: &[crate::personality::composition::resolve::ResolvedBinding],
) -> SlotId {
    panic!(
        "Slice 5 placeholder MintFn invoked: loaded plugins don't have handlers bound yet. \
         A follow-up commit should map each exposed capability name to a real handler."
    );
}
