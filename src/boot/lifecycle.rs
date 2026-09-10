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
//!   Phase 5  Log the legacy `consumes` entries (informational
//!            only; the resolver uses `[[requires]]` for the
//!            actual dependency graph).
//!   Phase 6  Activate runtime plugins in resolved order.
//!   Phase 7  Bring up the HTTP bridge and wait for Ctrl-C.
//!
//! Phase 7 in earlier versions exercised the caps with a demo
//! transcript; that has moved to `tests/{alpha,beta,gamma,delta,
//! epsilon}/`, so the boot here stops at "kernel + plugins
//! running".

use std::sync::Arc;

use serde_json::json;

use crate::capability::{CapabilityBudget, CapabilitySpace, CapKind, Slot};
use crate::boot::http_bridge::serve;
use crate::kernel::factory::CapabilityFactory;
use crate::kernel::manifest::{CapabilityDecl, PluginId, PluginManifest};
use crate::kernel::registry::Registry;
use crate::kernel::resolver::{resolve, ResolvedPlan};
use crate::plugins::{
    database::{database_plugin, handler as database_handler, DatabaseResource},
    embedder::{embedder_plugin, handler as embedder_handler, EmbedderResource},
    echo::{
        basic::{echo_plugin, handler as echo_handler, EchoResource},
        chain::{echo_chain_plugin, EchoChainResource},
        stream::{echo_stream_plugin, handler as echo_stream_handler, EchoStreamResource},
    },
    generator::{generator_plugin, handler as generator_handler, GeneratorResource},
    http::{handler as http_handler, http_plugin, HttpResource},
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
    "database",
    "embedder",
    "http",
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
        "database" => Some(database_plugin()),
        "embedder" => Some(embedder_plugin()),
        "http" => Some(http_plugin()),
        _ => None,
    }
}

/// Consistency check: every name in [`RUNTIME_PLUGINS`] must
/// have an arm in [`activator_for`]. The same is true for
/// [`mint_one_plugin`], but we can't verify that statically —
/// the closure types differ per plugin (each one binds a
/// different `Resource` type). Instead, [`mint_one_plugin`]
/// returns a loud `Err` for any name it doesn't recognise (the
/// fallthrough arm), and [`mint_runtime_plugins`] checks that
/// every `RUNTIME_PLUGINS` entry has a loaded manifest. The
/// three mechanisms together cover all four
/// "I added it here but forgot there" cases:
///
/// |                                | activator_for | mint_one_plugin | manifest loaded |
/// |--------------------------------|---------------|-----------------|-----------------|
/// | `dispatch_consistency`         | assert        | (runtime error) | (config check)  |
/// | `mint_runtime_plugins` config  | n/a           | (runtime error) | error           |
/// | `mint_one_plugin` fallthrough  | n/a           | error           | n/a             |
///
/// The `assert` is unconditional (runs in both debug and
/// release builds) because the activation loop's
/// `let Some(plugin) = activator_for(&plugin_id.name) else { continue; };`
/// would silently skip a missing runtime-plugin arm, leaving
/// minted slots un-bound to any cordis handler. Test-only
/// plugins in `plan.mint_order` still hit the `continue`
/// path (which is documented behaviour).
///
/// So the rule is simple: if you add a name to
/// `RUNTIME_PLUGINS`, add arms in both `activator_for` and
/// `mint_one_plugin`, and include the plugin's manifest in
/// `load_manifests()`.
fn dispatch_consistency() {
    for name in RUNTIME_PLUGINS {
        assert!(
            activator_for(name).is_some(),
            "RUNTIME_PLUGINS lists `{name}` but `activator_for` returns None"
        );
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
/// minted slot ids so [`ruin_runtime_plugins`] can walk
/// them in reverse mint order and revoke each one via
/// `cspace.revoke_tree`. The mapping is what makes the
/// teardown direction explicit: every provider cap that a
/// consumer's binding table points at is reachable as long as
/// the provider's slots are alive; revoking them in reverse
/// mint order means consumers' reachable entries become
/// invalid before their providers go away.
///
/// ## Configuration check (B1)
///
/// Before iterating, validate that every name in
/// [`RUNTIME_PLUGINS`] is backed by a loaded manifest. This
/// catches the misconfiguration where a name is added to
/// `RUNTIME_PLUGINS` but the corresponding `manifest()` fn
/// wasn't included in `load_manifests()`. Before this check
/// existed, the loop would silently skip the missing name and
/// the boot would succeed without the plugin.
async fn mint_runtime_plugins(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    manifests: &[PluginManifest],
) -> Result<std::collections::HashMap<PluginId, Vec<crate::capability::SlotId>>, Box<dyn std::error::Error>> {
    use std::collections::{BTreeMap, BTreeSet, HashMap};

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

    let mut minted: HashMap<PluginId, Vec<crate::capability::SlotId>> = HashMap::new();
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
        "generator" => mint_generator(ctx, factory, cspace, plan, m).await,
        "echo-chain" => mint_echo_chain(ctx, factory, cspace, plan, m).await,
        "http" => {
            // Phase 4 P4.1 — boot seeds the mock HTTP backend
            // with a single canned response so `GENERATOR_MODEL=http`
            // demos "just work". Real HTTP backends (Phase 5+)
            // drop in behind the same `Resource::invoke` shape
            // and ignore this seed.
            let res = http_handler();
            res.set("/llm/v1/complete", json!({
                "completion": "the kernel binds capabilities through the cspace"
            }));
            mint_simple::<HttpResource, _>(
                ctx, factory, m, CapKind::Sync, "slot:http", move |_, _| res.clone(),
            ).await
        }
        "database" => mint_simple::<DatabaseResource, _>(
            ctx, factory, m, CapKind::Sync, "slot:database", |_, _| database_handler(),
        ).await,
        "embedder" => mint_simple::<EmbedderResource, _>(
            ctx, factory, m, CapKind::Sync, "slot:embed", |_, _| embedder_handler(),
        ).await,
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
    // `mint_simple` is also called from `mint_generator`,
    // which DOES handle `[[requires]]` (it builds the model
    // with the reachable set and passes the closure here).
    // The original "silently drops requires" warning was
    // about plugins that *don't* set up reachable — those
    // should still fail loud. We now rely on per-plugin
    // mint fns (mint_generator, mint_echo_chain) to do the
    // setup, so mint_simple can be called safely from them.
    //
    // (The boot validation in `mint_runtime_plugins` still
    // refuses plugins that don't have an arm — see
    // `dispatch_consistency`.)
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


/// Phase 4 P4.1 — mint the generator plugin.
///
/// Generator has a `[[requires]]` dependency on `http`, so
/// `mint_simple` won't work (it doesn't honour requires —
/// `debug_assert!` would fire). We follow the same pattern as
/// `mint_echo_chain`:
///
/// 1. Look up the binding table entry for the `http` handle.
/// 2. Pick the model kind from `GENERATOR_MODEL` (mock /
///    markov / http).
/// 3. For `mock` and `markov`, build a self-contained model
///    (the env doesn't need to grant HTTP for those to work).
/// 4. For `http`, hand the cspace + reachable vec to
///    `HttpModel` so the dispatch happens through the
///    binding table.
/// 5. Mint the `generate` slot with the chosen model.
async fn mint_generator(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    m: &PluginManifest,
) -> Result<Vec<crate::capability::SlotId>, Box<dyn std::error::Error>> {
    use crate::plugins::generator::ModelKind;

    let model_kind = ModelKind::from_env();
    eprintln!(
        "[generator] selected model: {:?} (set GENERATOR_MODEL=mock|markov|http to override)",
        model_kind
    );

    // The binding for the `http` handle (if any). Mock /
    // Markov don't need it but still benefit from being told
    // what the env granted — useful for diagnostics.
    let reachable: Vec<crate::capability::Reachable> = plan
        .bindings
        .get(&m.plugin)
        .map(|bs| bs.iter().map(crate::capability::Reachable::from_binding).collect())
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

    mint_simple::<GeneratorResource, _>(
        ctx, factory, m, CapKind::Stream, "slot:generate",
        move |_, _| generator_handler(model_arc.clone()),
    ).await
}

/// Phase 3 P3.6 — Runtime Lifetime.
///
/// Tear down runtime plugins in **reverse** mint order — the
/// symmetric counterpart to `mint_runtime_plugins`. Each
/// plugin's minted slot ids are passed to `cspace.revoke_tree`,
/// which removes the slot and any descendants (derived caps
/// from `restrict`/`grant`). After this returns, every
/// runtime slot is freed; consumer binding entries pointing at
/// revoked caps return `None` from `cspace.lookup_by_name`.
///
/// The name mirrors `mint`: just as `mint` mints a typed
/// capability token, `ruin` reclaims it. The boot phase this
/// runs in is still called "shutdown" (see `[shutdown]`
/// log labels, `ShutdownStarted`/`ShutdownCompleted` graph
/// events) — those names describe the lifecycle phase, not
/// the action on each plugin's caps.
///
/// The order matters: consumers die **before** providers, so
/// any in-flight work the consumer was doing on the provider's
/// cap sees `Slot::capability() → None` rather than racing the
/// provider's teardown. For runtime plugins this is moot (they
/// mint fresh caps and don't derive), but the rule generalises
/// cleanly when later phases add real provider revocation
/// hooks.
async fn ruin_runtime_plugins(
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
        //   (per slot:)
        //     Revoked { slot, capability }
        //     RevokeTree { root, total }
        // — one PluginDeactivated per plugin, then per-slot
        // pairs of Revoked + RevokeTree. So a plugin with N
        // [[exposes]] blocks emits 1 + 2*N events from this
        // loop (plus any Revoked events from descendants
        // reached via revoke_tree). The per-slot granularity
        // gives audit logs a record of each individual cap
        // revocation; see ζ.16 for the single-slot case and
        // ζ.17 (multi_slot_plugin_shutdown) for the multi-slot
        // case.
        cspace.publish_event(
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

/// Phase 1 of boot prints every loaded manifest so operators
/// can confirm the resolver's input: which plugins loaded,
/// what they expose, what they require. Aligned columns + a
/// separator row make the dump scannable; `—` marks empty
/// requires (the alternative, e.g. an empty cell, reads as
/// a missing field).
fn print_manifests(manifests: &[PluginManifest]) {
    let mut rows: Vec<(String, String, String, String)> = Vec::with_capacity(manifests.len());
    for m in manifests {
        let name_ver = format!("{}@{}", m.plugin.name, m.plugin.version);
        let exposes = m
            .exposes
            .iter()
            .map(|c| c.name.clone())
            .collect::<Vec<_>>()
            .join(",");
        let contracts = m
            .exposes
            .iter()
            .map(|c| c.contract_name.clone())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        let requires = if m.requires.is_empty() {
            "—".to_string()
        } else {
            m.requires
                .iter()
                .map(|r| format!("{}→{}", r.name, r.contract))
                .collect::<Vec<_>>()
                .join(",")
        };
        rows.push((name_ver, exposes, contracts, requires));
    }

    // Column widths = max(header, longest cell), +1 for gutter.
    let headers = (
        "plugin".to_string(),
        "exposes".to_string(),
        "contract".to_string(),
        "requires".to_string(),
    );
    let col_width = |label: &str, cells: &[String]| -> usize {
        cells
            .iter()
            .map(|s| s.chars().count())
            .max()
            .unwrap_or(0)
            .max(label.chars().count())
            + 1
    };
    let wp = col_width(&headers.0, &rows.iter().map(|r| r.0.clone()).collect::<Vec<_>>());
    let we = col_width(&headers.1, &rows.iter().map(|r| r.1.clone()).collect::<Vec<_>>());
    let wc = col_width(&headers.2, &rows.iter().map(|r| r.2.clone()).collect::<Vec<_>>());
    let wr = col_width(&headers.3, &rows.iter().map(|r| r.3.clone()).collect::<Vec<_>>());

    println!("[manifest] loaded {} plugin(s):", manifests.len());
    println!(
        "  {:<wp$}{:<we$}{:<wc$}{:<wr$}",
        headers.0, headers.1, headers.2, headers.3,
    );
    println!(
        "  {}{}{}{}",
        "-".repeat(wp - 1),
        "-".repeat(we),
        "-".repeat(wc),
        "-".repeat(wr),
    );
    for (n, e, c, r) in &rows {
        println!(
            "  {:<wp$}{:<we$}{:<wc$}{:<wr$}",
            n, e, c, r,
        );
    }
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Phase 1: Parse manifests.
    let manifests = load_manifests(MANIFEST_DIR)?;
    print_manifests(&manifests);

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

    // Phase 5: legacy `consumes` log (informational only).
    //
    //     The resolver walks `[[requires]]` (Phase 3 P3.1).
    //     `[[consumes]]` is a back-compat field kept around so
    //     existing manifests can still describe their
    //     plugin-version-keyed dependencies for the audit log.
    //     We no longer treat a missing `consumes` provider as a
    //     boot error — doing so would create a confusing dual
    //     graph where `requires` succeeds but `consumes` fails
    //     and boot aborts anyway. Instead we print every entry
    //     and flag unprovided ones as a hint to migrate to
    //     `[[requires]]`.
    println!("\n[legacy] `consumes` entries (informational; resolver uses `requires`):");
    let provided_caps: std::collections::HashSet<String> = manifests
        .iter()
        .flat_map(|m| m.exposes.iter().map(|c| c.name.clone()))
        .collect();
    let mut legacy_unprovided: Vec<String> = Vec::new();
    for m in &manifests {
        for dep in &m.consumes {
            let ok = provided_caps.contains(&dep.capability);
            let mark = if ok { "✓" } else { "⚠" };
            println!(
                "  {} {}@{} consumes {} from {}@{} ({})",
                mark,
                m.plugin.name,
                m.plugin.version,
                dep.capability,
                dep.plugin,
                dep.version,
                if ok { "provided" } else { "unprovided — migrate to [[requires]]" }
            );
            if !ok {
                legacy_unprovided.push(format!(
                    "{} → {} (from {}@{})",
                    m.plugin.name, dep.capability, dep.plugin, dep.version
                ));
            }
        }
    }
    if !legacy_unprovided.is_empty() {
        println!(
            "[legacy] {} `consumes` entry/entries have no provider; boot continues \
             because the resolver uses `requires`. Consider migrating:\n  - {}",
            legacy_unprovided.len(),
            legacy_unprovided.join("\n  - ")
        );
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
                cspace.publish_event(
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
    cspace.publish_event(crate::capability::events::GraphEvent::ShutdownStarted);
    ruin_runtime_plugins(&cspace, &plan, &minted).await;
    cspace.publish_event(
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
        database::manifest as database_manifest,
        echo::{basic::manifest as echo_basic_manifest, chain::manifest as echo_chain_manifest, stream::manifest as echo_stream_manifest},
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
