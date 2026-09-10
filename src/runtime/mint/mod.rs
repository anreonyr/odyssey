//! Mint dispatch — typed `Capability<R>` for each runtime plugin.
//!
//! Phase 5: split from the Phase 4 `boot::lifecycle`. One
//! file per resource class — `simple` for plugins that mint
//! fresh caps with no cross-plugin dependency, `echo_chain`
//! / `generator` / `agent` for the three plugins whose mint
//! path consults the resolver's binding table.

pub mod agent;
pub mod echo_chain;
pub mod generator;
pub mod simple;

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::host::factory::CapabilityFactory;
use crate::host::manifest::PluginManifest;
use crate::host::resolver::ResolvedPlan;
use crate::kernel::ids::{PluginId, SlotId};
use crate::kernel::CapabilitySpace;

/// Runtime plugins that get the full mint + provide + activate
/// treatment at boot. Re-exported from `crate::runtime::activate`
/// to keep the dispatch tables in sync; the per-arm const
/// assertions there verify each name has a cordis activator.
pub use crate::runtime::activate::RUNTIME_PLUGINS;

/// Mint the runtime plugins in the order the resolver
/// produced, provide their slots to cordis, and return the
/// per-plugin minted slot ids so [`crate::runtime::teardown`]
/// can revoke each one in reverse mint order.
///
/// Test-only plugins (those not in [`RUNTIME_PLUGINS`]) are
/// skipped. Every name in `RUNTIME_PLUGINS` must be backed by
/// a loaded manifest — the configuration check below catches
/// the "added to RUNTIME_PLUGINS but forgot
/// `load_manifests()`" misconfiguration.
pub async fn mint_runtime_plugins(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    manifests: &[PluginManifest],
) -> Result<HashMap<PluginId, Vec<SlotId>>, Box<dyn std::error::Error>> {
    // A3: index manifests by PluginId for O(log n) per-plugin
    // lookup. Linear `.find()` per plugin was O(n) per lookup,
    // O(n²) over the full mint pass; with 50+ plugins that's
    // noticeable.
    let by_id: BTreeMap<PluginId, &PluginManifest> = manifests
        .iter()
        .map(|m| (m.plugin.clone(), m))
        .collect();

    // B1: every RUNTIME_PLUGINS entry must be backed by a
    // loaded manifest. Without this, a typo or a forgotten
    // `load_manifests()` entry would silently disappear.
    let loaded_names: BTreeSet<&str> =
        manifests.iter().map(|m| m.plugin.name.as_str()).collect();
    let missing: Vec<&&str> = RUNTIME_PLUGINS
        .iter()
        .filter(|n| !loaded_names.contains(*n))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "RUNTIME_PLUGINS lists {missing:?} but no loaded manifest publishes them; \
             either add the plugin to `load_manifests()` or remove it from RUNTIME_PLUGINS"
        )
        .into());
    }

    let mut minted: HashMap<PluginId, Vec<SlotId>> = HashMap::new();
    for plugin_id in &plan.mint_order {
        if !RUNTIME_PLUGINS.contains(&plugin_id.name.as_str()) {
            continue;
        }
        let m = by_id.get(plugin_id).ok_or_else(|| {
            format!(
                "resolver returned unknown plugin {}@{} (not in manifest index)",
                plugin_id.name, plugin_id.version
            )
        })?;
        let slots = mint_one_plugin(ctx, factory, cspace, plan, m).await?;
        if !slots.is_empty() {
            minted.insert(plugin_id.clone(), slots);
        }
    }
    Ok(minted)
}

/// Mint one runtime plugin. The dispatch table is fixed
/// because every runtime plugin is compiled into the host
/// binary and we know each one's typed `Resource` and
/// handler signature.
///
/// Returns the `Vec<SlotId>` minted for this plugin's
/// `[[exposes]]` blocks, in mint order. P3.6 teardown reads
/// this list to revoke each slot in reverse mint order.
pub async fn mint_one_plugin(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    m: &PluginManifest,
) -> Result<Vec<SlotId>, Box<dyn std::error::Error>> {
    use crate::kernel::CapKind;
    match m.plugin.name.as_str() {
        "echo" => {
            simple::mint_simple::<crate::plugins::echo::basic::EchoResource, _>(
                ctx, factory, m, CapKind::Sync, "slot:echo",
                |_, _| crate::plugins::echo::basic::handler(),
            )
            .await
        }
        "reverse" => {
            simple::mint_simple::<crate::plugins::reverse::ReverseResource, _>(
                ctx, factory, m, CapKind::Sync, "slot:reverse",
                |_, _| crate::plugins::reverse::handler(),
            )
            .await
        }
        "slow" => {
            simple::mint_simple::<crate::plugins::slow::SlowResource, _>(
                ctx, factory, m, CapKind::Sync, "slot:slow",
                |_, _| crate::plugins::slow::handler(),
            )
            .await
        }
        "sandbox" => {
            simple::mint_simple::<crate::plugins::sandbox::SandboxResource, _>(
                ctx, factory, m, CapKind::Sync, "slot:exec",
                |_, _| crate::plugins::sandbox::handler(),
            )
            .await
        }
        "echo_stream" => {
            simple::mint_simple::<crate::plugins::echo::stream::EchoStreamResource, _>(
                ctx, factory, m, CapKind::Stream, "slot:echo_stream",
                |_, _| crate::plugins::echo::stream::handler(),
            )
            .await
        }
        "generator" => generator::mint_generator(ctx, factory, cspace, plan, m).await,
        "agent" => agent::mint_agent(ctx, factory, cspace, plan, m).await,
        "echo-chain" => echo_chain::mint_echo_chain(ctx, factory, cspace, plan, m).await,
        "http" => {
            use serde_json::json;
            // Phase 4 P4.1 — boot seeds the mock HTTP backend
            // with a single canned response so `GENERATOR_MODEL=http`
            // demos "just work". Real HTTP backends (Phase 5+)
            // drop in behind the same `Resource::invoke` shape
            // and ignore this seed.
            let res = crate::plugins::http::handler();
            res.set(
                "/llm/v1/complete",
                json!({
                    "completion": "the kernel binds capabilities through the cspace"
                }),
            );
            simple::mint_simple::<crate::plugins::http::HttpResource, _>(
                ctx, factory, m, CapKind::Sync, "slot:http",
                move |_, _| res.clone(),
            )
            .await
        }
        "database" => {
            simple::mint_simple::<crate::plugins::database::DatabaseResource, _>(
                ctx, factory, m, CapKind::Sync, "slot:database",
                |_, _| crate::plugins::database::handler(),
            )
            .await
        }
        "embedder" => {
            simple::mint_simple::<crate::plugins::embedder::EmbedderResource, _>(
                ctx, factory, m, CapKind::Sync, "slot:embed",
                |_, _| crate::plugins::embedder::handler(),
            )
            .await
        }
        // B1: instead of silently returning an empty vec, fail
        // loudly. `mint_runtime_plugins` already checked that
        // every RUNTIME_PLUGINS name has a loaded manifest; if
        // we still reach this arm, the manifest loaded something
        // for which we forgot to write a mint match — surface
        // it instead of pretending the plugin had nothing to
        // mint.
        name => Err(format!(
            "mint_one_plugin: no mint arm for runtime plugin \"{name}\"; \
             add one to mint_one_plugin and keep RUNTIME_PLUGINS / activator_for / \
             mint_one_plugin in sync"
        )
        .into()),
    }
}
