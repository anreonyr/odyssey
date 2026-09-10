//! `mint_generator` — the mint path for the generator plugin.
//!
//! Phase 4 P4.1 — Generator has a `[[requires]]` dependency
//! on `http`, so `mint_simple` won't work. We follow the same
//! pattern as `mint_echo_chain`:
//!
//! 1. Look up the binding table entry for the `http` handle.
//! 2. Pick the model kind from `GENERATOR_MODEL` (mock /
//!    markov / http).
//! 3. For `mock` and `markov`, build a self-contained model
//!    (the env doesn't need to grant HTTP for those to work).
//! 4. For `http`, hand the cspace + reachable vec to
//!    `HttpModel` so the dispatch happens through the
//!    binding table.
//! 5. Mint the `generate` slot with the chosen model.

use std::sync::Arc;

use crate::host::factory::CapabilityFactory;
use crate::host::manifest::PluginManifest;
use crate::host::resolver::ResolvedPlan;
use crate::host::resolver::Reachable;
use crate::kernel::ids::SlotId;
use crate::kernel::CapabilitySpace;

use super::simple;

pub async fn mint_generator(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    m: &PluginManifest,
) -> Result<Vec<SlotId>, Box<dyn std::error::Error>> {
    use crate::kernel::CapKind;
    use crate::plugins::generator::ModelKind;

    let model_kind = ModelKind::from_env();
    eprintln!(
        "[generator] selected model: {:?} (set GENERATOR_MODEL=mock|markov|http to override)",
        model_kind
    );

    // The binding for the `http` handle (if any). Mock /
    // Markov don't need it but still benefit from being told
    // what the env granted — useful for diagnostics.
    let reachable: Vec<Reachable> = plan
        .bindings
        .get(&m.plugin)
        .map(|bs| bs.iter().map(Reachable::from_binding).collect())
        .unwrap_or_default();

    let model_arc: Arc<dyn crate::plugins::generator::Model> = match model_kind {
        ModelKind::Mock => ModelKind::Mock.build(),
        ModelKind::Markov => ModelKind::Markov.build(),
        ModelKind::Http => {
            if reachable.iter().all(|r| r.capability != "http_request") {
                eprintln!(
                    "[generator] WARNING: GENERATOR_MODEL=http but env didn't bind 'http' to any capability; \
                     falling back to Markov. Check that the http plugin is in RUNTIME_PLUGINS."
                );
                ModelKind::Markov.build()
            } else {
                ModelKind::build_http(cspace.clone(), reachable.clone())
            }
        }
    };

    simple::mint_simple::<crate::plugins::generator::GeneratorResource, _>(
        ctx,
        factory,
        m,
        CapKind::Stream,
        "slot:generate",
        move |_, _| crate::plugins::generator::handler(model_arc.clone()),
    )
    .await
}
