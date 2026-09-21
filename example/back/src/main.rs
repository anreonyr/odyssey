//! Odyssey basic example — boots the capability kernel + HTTP bridge
//! and serves the React app from `example/fore/dist`.
//!
//! The example binary lives at `example/backend/` to break the
//! cyclic dependency between the odyssey library and the
//! `odyssey-builtin` workspace member. Builtins depend on odyssey
//! (the library); the example depends on both. The library itself
//! stays plugin-free.
//!
//! The example assembles its plugins via 6 named bundles
//! (see `odyssey_builtin::bundles`):
//!
//! 1. `tool_caps()`        — `echo`, `database`
//! 2. `observers()`        — `agent_list`, `agent_describe`
//! 3. `inspectors_bundle()` — `inspector`, `schema_inspector`
//! 4. `model_providers()`  — `generator`, `embedder`, `reranker`
//! 5. `agent_bundle()`     — `agent`
//! 6. `bridge_bundle()`    — `http_bridge`
//!
//! Each bundle stamps its member manifests with a `BundleId`;
//! the kernel's resolver groups the boot diagram's mint order by
//! bundle (`ResolvedPlan::render` in
//! `crate/odyssey/src/personality/composition/resolve.rs`). The
//! resolver algorithm itself is unchanged — bundles are a
//! packaging concept at this layer, not a runtime dispatch
//! construct; `run_on` continues to receive a flat
//! `&[(PluginManifest, MintFn, RuinFn)]` slice.
//!
//! Cross-bundle `requires` work transparently: the resolver sees
//! the union of every bundle's manifests and produces a single
//! topological order. `agent_describe` (in `observers`) requires
//! `echo` and `database` (in `tool_caps`); `agent` (in `agent`)
//! requires `generator` and `embedder` (in `model-providers`).
//!
//! The HTTP bridge (`bridge_bundle`) is itself a regular plugin;
//! its mint reads `ODYSSEY_ADDR` / `ODYSSEY_NO_FRONTEND` /
//! `ODYSSEY_FRONTEND_DIST` from the environment (see
//! `example/back/src/bridge.rs`).

use odyssey::personality::lifecycle::run::run_on;
use odyssey_builtin::bundles;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Assemble the 11 builtin manifests via 6 named bundles.
    // The flatten is `Vec<Vec<(PluginManifest, MintFn, RuinFn)>>`
    // → `Vec<(PluginManifest, MintFn, RuinFn)>` — the slice
    // type `run_on` expects.
    let plugins = vec![
        bundles::tool_caps(),
        bundles::observers(),
        bundles::inspectors_bundle(),
        bundles::model_providers(),
        bundles::agent_bundle(),
        bundles::bridge_bundle(),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    // The orchestrator is plugin-agnostic. The HTTP bridge's
    // mint reads `ODYSSEY_ADDR` (default `127.0.0.1:3030`),
    // `ODYSSEY_NO_FRONTEND`, and `ODYSSEY_FRONTEND_DIST`
    // itself — see `example/back/src/bridge.rs`. We pass
    // nothing more than the plugin list.
    run_on(&plugins).await
}
