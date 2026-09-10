//! Odyssey — manifest-driven capability kernel boot.
//!
//! ## Possession model
//!
//! - The host owns the `CapabilitySpace` (seL4 CSpace).
//! - The factory mints typed `Capability<R>` and installs them at fresh
//!   slot ids.
//! - Plugins receive `Slot<R>` references via cordis inject — these are
//!   unforgeable handles to specific positions in the CSpace.
//!
//! ## Boot order
//!
//!   Phase 1  Parse manifests (split by runtime support)
//!   Phase 2  Provide core services (cspace, factory, registry)
//!   Phase 3  Resolve the capability dependency graph (Phase 3 P3.1)
//!            — topological order + per-plugin binding table.
//!   Phase 4  Mint typed tokens in resolved order for runtime
//!            plugins; provide slots to cordis. Test-only plugins
//!            (counter, broker, channel, agent) are resolved and
//!            registered but not minted at boot — they're
//!            exercised by the test crates.
//!   Phase 5  Validate the legacy `consumes` graph (back-compat).
//!   Phase 6  Activate runtime plugins in resolved order.
//!   Phase 7  Bring up the HTTP bridge and wait for Ctrl-C.
//!
//! Phase 7 in earlier versions exercised the caps with a demo
//! transcript; that has moved to `tests/{alpha,beta,gamma,delta,
//! epsilon}/`, so the boot here stops at "kernel + plugins
//! running".

use std::sync::Arc;

use crate::capability::{CapabilityBudget, CapabilitySpace, CapKind, Slot};
use crate::boot::http_bridge::serve;
use crate::kernel::factory::CapabilityFactory;
use crate::kernel::manifest::{CapabilityDecl, PluginId, PluginManifest};
use crate::kernel::registry::Registry;
use crate::kernel::resolver::{resolve, ResolvedPlan};
use crate::plugins::{
    echo::{
        basic::{echo_plugin, handler as echo_handler, EchoResource},
        chain::{echo_chain_plugin, EchoChainResource},
        stream::{echo_stream_plugin, handler as echo_stream_handler, EchoStreamResource},
    },
    generator::{generator_plugin, handler as generator_handler, GeneratorResource},
    reverse::{handler as reverse_handler, reverse_plugin, ReverseResource},
    sandbox::{handler as sandbox_handler, sandbox_plugin, SandboxResource},
    slow::{handler as slow_handler, slow_plugin, SlowResource},
};

const MANIFEST_DIR: &str = "src/plugins";

/// Plugin names that get the full mint + provide + activate
/// treatment at boot. Test-only plugins (under `src/plugins/
/// test_only/`) are skipped by both the manifest walker and
/// the dispatch in this file — they're reached through the
/// test crates directly via `factory.mint`.
///
/// The typed-mint match in [`mint_one_plugin`] and the
/// activator match in the boot loop both reference this list.
/// [`dispatch_consistency`] is a debug_assert that they stay
/// in sync; if you add a plugin here, you must add both an
/// arm in `mint_one_plugin` and an arm in the activator match.
const RUNTIME_PLUGINS: &[&str] = &[
    "echo",
    "reverse",
    "slow",
    "sandbox",
    "echo_stream",
    "generator",
    "echo-chain",
];

/// Returns the cordis `Plugin` activator for a runtime plugin
/// by name, or `None` if the name isn't a runtime plugin.
///
/// Mirrors [`mint_one_plugin`] — every name in
/// [`RUNTIME_PLUGINS`] must have both an arm here and an arm
/// there. Enforced by [`dispatch_consistency`].
fn activator_for(name: &str) -> Option<Arc<dyn cordis::Plugin>> {
    match name {
        "echo" => Some(echo_plugin()),
        "reverse" => Some(reverse_plugin()),
        "slow" => Some(slow_plugin()),
        "sandbox" => Some(sandbox_plugin()),
        "echo_stream" => Some(echo_stream_plugin()),
        "generator" => Some(generator_plugin()),
        "echo-chain" => Some(echo_chain_plugin()),
        _ => None,
    }
}

/// Debug-only consistency check: every name in
/// [`RUNTIME_PLUGINS`] must have an arm in [`activator_for`].
///
/// We can't statically verify the typed-mint match in
/// [`mint_one_plugin`] the same way (the closure types differ
/// per plugin), but the `activator_for` arm-list mirrors it
/// one-to-one. If you add a name to `RUNTIME_PLUGINS`, add
/// arms in both `activator_for` and `mint_one_plugin`.
#[allow(dead_code)]
fn dispatch_consistency() {
    #[cfg(debug_assertions)]
    {
        for name in RUNTIME_PLUGINS {
            debug_assert!(
                activator_for(name).is_some(),
                "RUNTIME_PLUGINS lists `{name}` but `activator_for` returns None"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Mint dispatch
// ---------------------------------------------------------------------------

/// Mint the runtime plugins in the order the resolver produced,
/// provide their slots to cordis, and activate them. Test-only
/// plugins are skipped (they're not in [`RUNTIME_PLUGINS`]).
///
/// Phase 3 P3.6 — Runtime Lifetime. Returns the per-plugin
/// minted slot ids so [`shutdown_runtime_plugins`] can walk
/// them in reverse mint order and revoke each one via
/// `cspace.revoke_tree`. The mapping is what makes the
/// teardown direction explicit: every provider cap that a
/// consumer's binding table points at is reachable as long as
/// the provider's slots are alive; revoking them in reverse
/// mint order means consumers' reachable entries become
/// invalid before their providers go away.
async fn mint_runtime_plugins(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    manifests: &[PluginManifest],
) -> Result<std::collections::HashMap<PluginId, Vec<crate::capability::SlotId>>, Box<dyn std::error::Error>> {
    use std::collections::HashMap;
    let mut minted: HashMap<PluginId, Vec<crate::capability::SlotId>> = HashMap::new();
    for plugin_id in &plan.mint_order {
        if !RUNTIME_PLUGINS.contains(&plugin_id.name.as_str()) {
            continue;
        }
        let m = manifests
            .iter()
            .find(|m| &m.plugin == plugin_id)
            .ok_or_else(|| format!("resolver returned unknown plugin {}", plugin_id.name))?;
        let slots = mint_one_plugin(ctx, factory, cspace, plan, m).await?;
        if !slots.is_empty() {
            minted.insert(plugin_id.clone(), slots);
        }
    }
    Ok(minted)
}

/// Mint one runtime plugin. The dispatch table is fixed because
/// every runtime plugin is compiled into the host binary and we
/// know each one's typed `Resource` and handler signature.
///
/// Returns the `Vec<SlotId>` minted for this plugin's
/// `[[exposes]]` blocks, in mint order. P3.6 teardown reads
/// this list to revoke each slot in reverse mint order.
async fn mint_one_plugin(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    m: &PluginManifest,
) -> Result<Vec<crate::capability::SlotId>, Box<dyn std::error::Error>> {
    match m.plugin.name.as_str() {
        "echo" => mint_simple::<EchoResource, _>(
            ctx, factory, m, CapKind::Sync, "slot:echo", |_, _| echo_handler(),
        ).await,
        "reverse" => mint_simple::<ReverseResource, _>(
            ctx, factory, m, CapKind::Sync, "slot:reverse", |_, _| reverse_handler(),
        ).await,
        "slow" => mint_simple::<SlowResource, _>(
            ctx, factory, m, CapKind::Sync, "slot:slow", |_, _| slow_handler(),
        ).await,
        "sandbox" => mint_simple::<SandboxResource, _>(
            ctx, factory, m, CapKind::Sync, "slot:exec", |_, _| sandbox_handler(),
        ).await,
        "echo_stream" => mint_simple::<EchoStreamResource, _>(
            ctx, factory, m, CapKind::Stream, "slot:echo_stream", |_, _| echo_stream_handler(),
        ).await,
        "generator" => mint_simple::<GeneratorResource, _>(
            ctx, factory, m, CapKind::Stream, "slot:generate", |_, _| generator_handler(),
        ).await,
        "echo-chain" => mint_echo_chain(ctx, factory, cspace, plan, m).await,
        _ => Ok(Vec::new()), // unknown runtime plugin name; skip
    }
}

/// Mint a "simple" plugin: every `[[exposes]]` block produces
/// one typed `Capability<R>`, installed under `slot_key`.
/// Returns the freshly minted slot ids so the caller can
/// revoke them at teardown.
async fn mint_simple<R, F>(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    m: &PluginManifest,
    kind: CapKind,
    slot_key: &'static str,
    handler_for: F,
) -> Result<Vec<crate::capability::SlotId>, Box<dyn std::error::Error>>
where
    R: crate::capability::Resource + 'static,
    F: Fn(&CapabilityDecl, &PluginId) -> Arc<R>,
{
    let mut slots = Vec::with_capacity(m.exposes.len());
    for cap in &m.exposes {
        let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
        let handler = handler_for(cap, &m.plugin);
        let slot_id = factory.mint::<R>(kind, cap, &m.plugin, budget, handler);
        let slot = Slot::<R>::new(factory.space().clone(), slot_id);
        ctx.provide(slot_key, slot).await
            .map_err(|e| format!("provide {slot_key}: {e}"))?;
        println!("    {:<14} → slot={slot_id}  contract={}", cap.name, cap.contract_name);
        slots.push(slot_id);
    }
    Ok(slots)
}

/// echo-chain is the only runtime plugin that needs a
/// cross-plugin capability at mint time: it closes over the
/// typed `Capability<EchoResource>`. The cap is delivered via
/// the **resolved binding table** — echo-chain's manifest
/// declares `[[requires]] name="echo" contract="echo"` and the
/// resolver walks it to find which provider fulfils the
/// contract. `plan.bindings[m.plugin]` carries that mapping;
/// we read the binding's `capability` field and look up the cap
/// by that name in cspace. The provider was minted earlier in
/// `plan.mint_order` (topologically), so the cap is already in
/// cspace by the time we reach this branch.
async fn mint_echo_chain(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    m: &PluginManifest,
) -> Result<Vec<crate::capability::SlotId>, Box<dyn std::error::Error>> {
    // 1) Find the binding for echo-chain's `echo` handle.
    let bindings = plan
        .bindings
        .get(&m.plugin)
        .ok_or_else(|| format!("echo-chain: no bindings for {} in plan", m.plugin.name))?;
    let echo_binding = bindings
        .iter()
        .find(|b| b.handle == "echo")
        .ok_or_else(|| {
            format!(
                "echo-chain: no binding for handle \"echo\" (requires = {:?})",
                m.requires
            )
        })?;

    // 2) Look up the cap by the binding's `capability` field.
    //    This is the same lookup the resolver performed at plan
    //    time — by following it again here, the runtime path
    //    remains driven entirely by the plan; if the resolver
    //    changed how it picks providers (e.g. version ranges),
    //    echo-chain automatically tracks.
    let echo_cap = cspace
        .lookup_by_name(&echo_binding.capability)
        .ok_or_else(|| {
            format!(
                "echo-chain: capability \"{}\" (contract {}) not in cspace",
                echo_binding.capability, echo_binding.contract
            )
        })?;
    let typed = echo_cap
        .as_any()
        .downcast_ref::<crate::capability::Capability<EchoResource>>()
        .ok_or_else(|| {
            format!(
                "echo-chain: capability \"{}\" has wrong type (expected Capability<EchoResource>)",
                echo_binding.capability
            )
        })?;
    let typed_arc = Arc::new(typed.clone());

    let mut slots = Vec::with_capacity(m.exposes.len());
    for cap in &m.exposes {
        let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
        let slot_id = factory.mint::<EchoChainResource>(
            CapKind::Sync,
            cap,
            &m.plugin,
            budget,
            crate::plugins::echo::chain::handler(typed_arc.clone()),
        );
        let slot = Slot::<EchoChainResource>::new(cspace.clone(), slot_id);
        ctx.provide("slot:echo_chain", slot).await
            .map_err(|e| format!("provide slot:echo_chain: {e}"))?;
        println!("    {:<14} → slot={slot_id}  contract={}", cap.name, cap.contract_name);
        slots.push(slot_id);
    }
    Ok(slots)
}

/// Phase 3 P3.6 — Runtime Lifetime.
///
/// Tear down runtime plugins in **reverse** mint order. Each
/// plugin's minted slot ids are passed to
/// `cspace.revoke_tree`, which removes the slot and any
/// descendants (derived caps from `restrict`/`grant`). After
/// this returns, every runtime slot is freed; consumer binding
/// entries pointing at revoked caps return `None` from
/// `cspace.lookup_by_name`.
///
/// The order matters: consumers die **before** providers, so
/// any in-flight work the consumer was doing on the provider's
/// cap sees `Slot::capability() → None` rather than racing the
/// provider's teardown. For runtime plugins this is moot (they
/// mint fresh caps and don't derive), but the rule generalises
/// cleanly when later phases add real provider revocation
/// hooks.
async fn shutdown_runtime_plugins(
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    minted: &std::collections::HashMap<PluginId, Vec<crate::capability::SlotId>>,
) {
    println!("\n[shutdown] tearing down runtime plugins (reverse mint order):");
    for plugin_id in plan.mint_order.iter().rev() {
        let Some(slot_ids) = minted.get(plugin_id) else {
            continue;
        };
        // Phase 3 P3.7 — emit PluginDeactivated before
        // revoking. The subsequent `revoke_tree` calls emit
        // Revoked + RevokeTree events from cspace. Order:
        //   PluginDeactivated { plugin }
        //   Revoked { slot, capability }
        //   RevokeTree { root, total }
        // — one PluginDeactivated per plugin, then N Revoked +
        // 1 RevokeTree per plugin.
        let _ = cspace.events().publish(
            crate::capability::events::GraphEvent::PluginDeactivated {
                plugin: plugin_id.clone(),
            },
        );
        let mut total_revoked = 0usize;
        for slot_id in slot_ids {
            let n = cspace.revoke_tree(*slot_id);
            total_revoked += n;
        }
        if total_revoked > 0 || !slot_ids.is_empty() {
            println!(
                "  ✓ {}@{}  revoked {} slot(s)",
                plugin_id.name, plugin_id.version, total_revoked
            );
        }
    }
    let remaining = cspace.len();
    println!("[shutdown] cspace remaining slots: {remaining}");
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Phase 1: Parse manifests.
    let manifests = load_manifests(MANIFEST_DIR)?;
    println!("[manifest] loaded {} plugin(s):", manifests.len());
    for m in &manifests {
        println!(
            "  - {}@{}  exposes={}  contracts={}  requires={}",
            m.plugin.name,
            m.plugin.version,
            m.exposes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(","),
            m.exposes
                .iter()
                .map(|c| c.contract_name.as_str())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(","),
            if m.requires.is_empty() {
                "—".to_string()
            } else {
                m.requires
                    .iter()
                    .map(|r| format!("{}→{}", r.name, r.contract))
                    .collect::<Vec<_>>()
                    .join(",")
            },
        );
    }

    // Phase 2: Provide core services.
    let ctx = cordis::Context::new();
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace.clone());
    let registry = Arc::new(Registry::default());

    ctx.provide("capability_space", cspace.clone()).await?;
    ctx.provide("capability_factory", factory.clone()).await?;
    ctx.provide("registry", registry.clone()).await?;
    println!("[main] core services provided");

    // Phase 3: Resolve the capability dependency graph.
    //
    //     Phase 3 P3.1 — capability injection. The resolver reads
    //     each manifest's `[[requires]] contract` and matches
    //     against another manifest's `[[exposes]] contract_name`.
    //     Output: a topological mint order and a per-plugin
    //     binding table.
    let plan: ResolvedPlan = resolve(&manifests).map_err(|e| format!("resolver: {e}"))?;
    println!("\n[resolver]\n{}", plan.render());

    // Phase 4: Mint runtime plugins in resolved order. P3.6
    // collects the minted slot ids so the shutdown phase
    // can revoke each one in reverse mint order.
    println!("[mint] runtime plugins (in resolved order):");
    let minted = mint_runtime_plugins(&ctx, &factory, &cspace, &plan, &manifests).await?;

    // Phase 5: Validate legacy `consumes` graph (back-compat).
    println!("\n[graph] validating legacy `consumes` dependencies:");
    let provided_caps: std::collections::HashSet<String> = manifests
        .iter()
        .flat_map(|m| m.exposes.iter().map(|c| c.name.clone()))
        .collect();
    let mut missing: Vec<(String, String, String)> = Vec::new();
    for m in &manifests {
        for dep in &m.consumes {
            let ok = provided_caps.contains(&dep.capability);
            let mark = if ok { "✓" } else { "✗" };
            println!(
                "  {} {}@{} consumes {} from {}@{} ({})",
                mark,
                m.plugin.name,
                m.plugin.version,
                dep.capability,
                dep.plugin,
                dep.version,
                if ok { "provided" } else { "MISSING" }
            );
            if !ok {
                missing.push((
                    m.plugin.name.clone(),
                    dep.plugin.clone(),
                    dep.capability.clone(),
                ));
            }
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "missing capability providers: {}",
            missing
                .iter()
                .map(|(consumer, provider, cap)| format!("{consumer} needs {cap} from {provider}"))
                .collect::<Vec<_>>()
                .join("; ")
        )
        .into());
    }

    // Register manifests.
    for m in &manifests {
        registry.register(m.clone())?;
    }

    // Phase 6: Activate runtime plugins in resolved order.
    println!("\n[plugins] activating (in resolved order):");
    dispatch_consistency();
    for plugin_id in &plan.mint_order {
        let Some(plugin) = activator_for(&plugin_id.name) else {
            continue;
        };
        let handle = ctx.plugin(plugin, None);
        match handle.join().await {
            Ok(()) => {
                println!("  ✓ {}@{} activated", plugin_id.name, plugin_id.version);
                // Phase 3 P3.7 — emit PluginActivated. The cap
                // itself already published `Minted` at factory
                // time; this marks the plugin's lifecycle event
                // (the cordis handler returned Ok).
                let _ = cspace.events().publish(
                    crate::capability::events::GraphEvent::PluginActivated {
                        plugin: plugin_id.clone(),
                    },
                );
            }
            Err(e) => eprintln!("  ✗ {}@{} failed: {e}", plugin_id.name, plugin_id.version),
        }
    }

    // Phase 7: HTTP bridge + wait.
    let ctx_clone = ctx.clone();
    let cspace_clone = cspace.clone();
    let server_handle = tokio::spawn(async move {
        serve(
            "127.0.0.1:3030".parse().unwrap(),
            ctx_clone,
            cspace_clone,
            async {
                let _ = tokio::signal::ctrl_c().await;
            },
        )
        .await;
    });
    eprintln!("\n[main] HTTP bridge up — open http://127.0.0.1:3030/");
    eprintln!("[main] press Ctrl-C to stop");

    let _ = server_handle.await;
    eprintln!("[main] shutting down");

    // Phase 8 (P3.6 + P3.7): tear down runtime plugins in
    // reverse mint order, revoking each plugin's minted slot
    // via `cspace.revoke_tree`. Before the teardown starts we
    // emit `ShutdownStarted`; each revoke emits `Revoked` +
    // `RevokeTree` events (from cspace); after teardown we
    // emit `ShutdownCompleted { remaining_slots }`. Plugin
    // lifecycle events (`PluginDeactivated`) accompany each
    // per-plugin revoke.
    let _ = cspace.events().publish(
        crate::capability::events::GraphEvent::ShutdownStarted,
    );
    shutdown_runtime_plugins(&cspace, &plan, &minted).await;
    let _ = cspace.events().publish(
        crate::capability::events::GraphEvent::ShutdownCompleted {
            remaining_slots: cspace.len(),
        },
    );

    ctx.stop().await;

    Ok(())
}

/// Collect every runtime plugin's manifest.
///
/// Phase 3 replaces the toml walker with explicit enumeration
/// of each plugin's `manifest()` function. The toml files for
/// runtime plugins no longer exist; test_only plugins are
/// reached by the test crates directly via `factory.mint`, not
/// through boot.
///
/// We keep the `dir` parameter for back-compat with the
/// previous signature; it's now ignored. (A future phase may
/// add a `--from-toml` flag that re-reads `*.toml` for plugins
/// loaded via WASM / cdylib / subprocess loaders, in which
/// case `dir` would point at the wire-format drop-in.)
fn load_manifests(_dir: &str) -> Result<Vec<PluginManifest>, Box<dyn std::error::Error>> {
    use crate::plugins::{
        echo::{basic::manifest as echo_basic_manifest, chain::manifest as echo_chain_manifest, stream::manifest as echo_stream_manifest},
        generator::manifest as generator_manifest,
        reverse::manifest as reverse_manifest,
        sandbox::manifest as sandbox_manifest,
        slow::manifest as slow_manifest,
    };

    let manifests = [
        echo_basic_manifest(),
        echo_chain_manifest(),
        echo_stream_manifest(),
        generator_manifest(),
        reverse_manifest(),
        sandbox_manifest(),
        slow_manifest(),
    ];
    let mut out: Vec<PluginManifest> = manifests.iter().map(|m| (*m).clone()).collect();
    out.sort_by(|a, b| a.plugin.name.cmp(&b.plugin.name));
    Ok(out)
}
