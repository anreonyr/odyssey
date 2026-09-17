//! Personality / lifecycle / run — the top-level orchestrator.
//!
//! Phase 8 design: dispatches by plugin name through a
//! per-builtin typed mint path. The trait-object `Arc<dyn
//! Builtin>` design hit a wall — the kernel's
//! `CapabilityFactory::mint<R>` is generic over `R`, and a
//! trait object can't dispatch into a generic call.
//!
//! Phase 10 cleanup: the `Mint` trait that Phase 9 introduced
//! for per-plugin dispatch was pure forwarding boilerplate
//! (each builtin's `impl Mint for XBuiltin` was a one-line
//! forward to the inherent `pub fn mint(...)`). Replaced with
//! a `MintFn` function-pointer registry keyed by plugin name.
//!
//! Phase 11: paired `RuinFn` registry — symmetric to `MintFn`.
//! Builtins expose a colocated `fn register() -> (PluginManifest,
//! MintFn, RuinFn)` helper; the third tuple element is the hook
//! the orchestrator fires per plugin while the slots are still
//! live, so a future persistence builtin flushing to disk or a
//! streaming builtin draining its producer task would override
//! the default. Today every builtin's `RuinFn` is `default_ruin`.
//!
//! The hook owns the revoke and returns how many slots it
//! removed. It cannot both delegate the revoke and have the
//! orchestrator count it afterwards: that second pass always
//! found the slot already gone and reported `0`. The
//! orchestrator reports the hook's number instead, and revokes
//! the slots itself only when the hook failed or panicked — the
//! one case where the revoke did not run.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::capability::enforce::quota::CapabilityBudget;
use crate::capability::enforce::space::CapabilitySpace;
use crate::core::clock::clock::SystemClock;
use crate::core::identity::ids::{PluginId, SlotId};
use crate::core::identity::kind::CapKind;
use crate::core::manifest::manifest::{CapabilityDecl, PluginManifest};
use crate::personality::composition::resolve::{ResolvedBinding, ResolvedPlan, resolve};
use crate::personality::lifecycle::lifecycle_event::{LifecycleEvent, LifecycleEventBus};
use crate::personality::lifecycle::mint::CapabilityFactory;
use crate::personality::lifecycle::serve::spawn_http_bridge;

/// Typed mint entry point — the kernel's
/// `CapabilityFactory::mint<R>` is generic over `R` so this is
/// a free function pointer (not a trait object). Each builtin
/// exposes a non-capturing closure of this shape via its
/// `fn register() -> (PluginManifest, MintFn, RuinFn)` helper.
/// The orchestrator dispatches by `plugin_id.name.as_str()`
/// against a registry of these.
///
/// `bindings` is the *minting plugin's own* row of
/// `ResolvedPlan::bindings` — the capabilities its `requires`
/// resolved to, as `(handle, provider, capability)` triples. It
/// is empty for a plugin that declares no dependencies. A
/// builtin that has no use for its binding table ignores it;
/// the agent builtin is the one that keeps it, and that is the
/// only way the table reaches a running plugin. Injection
/// happens here rather than at call time because minting walks
/// `plan.mint_order`, so a consumer is always minted after the
/// providers it binds to.
pub type MintFn = fn(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    decl: &CapabilityDecl,
    kind: CapKind,
    budget: CapabilityBudget,
    bindings: &[ResolvedBinding],
) -> SlotId;

/// Per-plugin teardown hook. Called by the orchestrator *before*
/// any orchestrator-side revoke, so the hook still sees its own
/// live slots (a future builtin with custom teardown — flushing
/// state, draining a producer task — needs to read its slot's
/// state before the cspace evicts it).
///
/// The hook owns the revoke. It returns how many slots it
/// removed, and the orchestrator reports that number rather than
/// revoking a second time to count. The previous shape had the
/// orchestrator serially revoke every slot *after* the hook had
/// already revoked them via `default_ruin`, so `revoke_tree`
/// always found the slot gone and the "revoked N slot(s)" line
/// always printed `0`.
///
/// Sync today (no current builtin needs to await on teardown). If
/// a future builtin needs async drain, promote the return to
/// `impl Future<Output = Result<usize, String>>` and add `.await`
/// at the call site in `run` — the signature change is local to
/// this file.
pub type RuinFn = fn(cspace: &CapabilitySpace, slot_ids: &[SlotId]) -> Result<usize, String>;

/// Default teardown — revoke every minted slot of one plugin and
/// report the total removed. Exposed so every builtin's
/// `register()` can reference the same symbol instead of
/// duplicating the loop body.
pub fn default_ruin(cspace: &CapabilitySpace, slot_ids: &[SlotId]) -> Result<usize, String> {
    let mut revoked = 0usize;
    for id in slot_ids {
        revoked += cspace.revoke_tree(*id);
    }
    Ok(revoked)
}

/// The bridge address `run` uses when the caller does not pick one.
pub const DEFAULT_BRIDGE_ADDR: &str = "127.0.0.1:3030";

/// Top-level entry point on the default bridge address. Boots the
/// kernel, resolves the provided `(manifest, mint_fn, ruin_fn)`
/// triples, mints each capability via the matching `mint_fn`,
/// serves HTTP, and tears down on Ctrl-C. The default
/// `run` does **not** serve the React frontend — that's an
/// example concern; pass the path to `run_on` if you need it.
pub async fn run(
    plugins: &[(PluginManifest, MintFn, RuinFn)],
) -> Result<(), Box<dyn std::error::Error>> {
    run_on(DEFAULT_BRIDGE_ADDR.parse().unwrap(), None, plugins).await
}

/// `run` with an explicit bridge address.
///
/// The address is a parameter rather than a constant so a test can
/// give its spawned server a port of its own. Without that, a test
/// that starts the example binary either fights whatever already
/// holds the fixed port, or — worse — silently connects to it and
/// asserts against a build that is not the one under test.
///
/// `frontend_dist` is the path to the React app's built `dist/`
/// directory. When `Some`, the HTTP bridge also serves the app
/// at `/`, `/assets/*`, and as a SPA fallback; when `None`, the
/// API surface still works but UI routes return 503. The example
/// binary passes `Some("../../frontend/dist")` from its own
/// `CARGO_MANIFEST_DIR`; tests pass `None` since they only need
/// the API.
pub async fn run_on(
    addr: std::net::SocketAddr,
    frontend_dist: Option<&std::path::Path>,
    plugins: &[(PluginManifest, MintFn, RuinFn)],
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup — the resolver takes manifests alone; the
    // dispatch fns stay behind in the registry.
    let manifests: Vec<PluginManifest> = plugins.iter().map(|(m, _, _)| m.clone()).collect();
    println!("[boot] loaded {} builtin(s)", manifests.len());

    let cspace: CapabilitySpace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    // 2. Resolve.
    let plan: ResolvedPlan = resolve(&manifests).map_err(|e| format!("resolver: {e}"))?;
    println!("\n[resolver]\n{}", plan.render());

    // 3. Mint in resolved order — dispatch by plugin name through
    // the registry.
    let minted = mint_from_registry(&factory, &plan, plugins).await?;
    // 4. Serve.
    let server_handle = spawn_http_bridge(addr, cspace.clone(), frontend_dist);
    eprintln!("\n[main] HTTP bridge up — open http://{addr}/");
    eprintln!("[main] press Ctrl-C to stop");
    let _ = server_handle.await;

    // 5. Teardown — per-plugin `RuinFn` (default or custom)
    // fires before `cspace.revoke_tree` per slot so the hook
    // sees live slots. Catch_unwind protects the orchestrator
    // from a panicking builtin; the cspace revoke still runs
    // afterwards so we don't leak slots.
    //
    // Slice 3 PluginCspace cleanup: after the `RuinFn` fires
    // against the global cspace, the orchestrator drains the
    // plugin's own `PluginCspace` via
    // `factory.reclaim_plugin`. A builtin that mints into its
    // per-plugin cspace and grants a derived slot into the
    // global cspace has TWO live slots — the global one (in
    // `minted`, revoked by `default_ruin`) and the local one
    // (only in the plugin's `PluginCspace`, previously leaked).
    // `RuinFn` keeps its signature untouched; the cleanup runs
    // here at the call site, keyed off the resolved plugin
    // id and the factory's `plugin_cspaces` map.
    let lifecycle = LifecycleEventBus::new();
    let _ = lifecycle.publish(LifecycleEvent::ShutdownStarted);
    ruin_via_registry(&factory, &cspace, &plan, &minted, plugins);
    let _ = lifecycle.publish(LifecycleEvent::ShutdownCompleted {
        remaining_slots: cspace.len(),
    });
    Ok(())
}

/// One registry table, walked by the same key for both mint and
/// ruin. The previous shape derived a `by_id` manifest map from
/// `plugins` and a separate `by_name` dispatch map, then guarded
/// both lookups — unguardable in practice, because every id in
/// `plan.mint_order` came from the same `plugins` slice, so the
/// guards could never fire. Building the table once removes the
/// re-derivation and the unreachable error branches with it.
struct RegistryEntry<'a> {
    manifest: &'a PluginManifest,
    mint_fn: MintFn,
    ruin_fn: RuinFn,
}

fn plugin_registry<'a>(
    plugins: &'a [(PluginManifest, MintFn, RuinFn)],
) -> Result<BTreeMap<&'a str, RegistryEntry<'a>>, Box<dyn std::error::Error>> {
    let mut by_name: BTreeMap<&str, RegistryEntry<'_>> = BTreeMap::new();
    for (manifest, mint_fn, ruin_fn) in plugins {
        let inserted = by_name
            .insert(
                manifest.plugin.name.as_str(),
                RegistryEntry {
                    manifest,
                    mint_fn: *mint_fn,
                    ruin_fn: *ruin_fn,
                },
            )
            .is_none();
        if !inserted {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "duplicate plugin name in registry: `{}` appears more than once",
                    manifest.plugin.name
                ),
            )));
        }
    }
    Ok(by_name)
}

async fn mint_from_registry(
    factory: &CapabilityFactory,
    plan: &ResolvedPlan,
    plugins: &[(PluginManifest, MintFn, RuinFn)],
) -> Result<HashMap<PluginId, Vec<SlotId>>, Box<dyn std::error::Error>> {
    let by_name = plugin_registry(plugins)?;

    let mut minted: HashMap<PluginId, Vec<SlotId>> = HashMap::new();
    for plugin_id in &plan.mint_order {
        let entry = &by_name[plugin_id.name.as_str()];
        let bindings: &[ResolvedBinding] = plan
            .bindings
            .get(plugin_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut plugin_slots = Vec::with_capacity(entry.manifest.exposes.len());
        for decl in &entry.manifest.exposes {
            let budget = CapabilityBudget::new(entry.manifest.timeout_ms.unwrap_or(5000));
            let slot_id = (entry.mint_fn)(factory, plugin_id, decl, decl.kind, budget, bindings);
            plugin_slots.push(slot_id);
        }
        minted.insert(plugin_id.clone(), plugin_slots);
    }
    Ok(minted)
}

fn ruin_via_registry(
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    minted: &HashMap<PluginId, Vec<SlotId>>,
    plugins: &[(PluginManifest, MintFn, RuinFn)],
) {
    let by_name = match plugin_registry(plugins) {
        Ok(by_name) => by_name,
        Err(e) => {
            eprintln!("[shutdown] registry is inconsistent: {e}");
            return;
        }
    };

    println!("\n[shutdown] tearing down runtime plugins (reverse mint order):");
    for plugin_id in plan.mint_order.iter().rev() {
        let Some(slot_ids) = minted.get(plugin_id) else {
            continue;
        };
        let entry = &by_name[plugin_id.name.as_str()];
        // The registered RuinFn owns the revoke and reports how
        // many slots it removed. A panicking hook is a contributor
        // bug, and it is also the one case where the revoke did not
        // run — so the orchestrator revokes those slots itself
        // rather than leaving them live.
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (entry.ruin_fn)(cspace, slot_ids)
        }));
        let global_revoked = match outcome {
            Ok(Ok(revoked)) => revoked,
            Ok(Err(message)) => {
                eprintln!("[shutdown] {} RuinFn failed: {message}", plugin_id.name);
                slot_ids
                    .iter()
                    .map(|slot_id| cspace.revoke_tree(*slot_id))
                    .sum()
            }
            Err(payload) => {
                eprintln!("[shutdown] {} RuinFn panicked: {payload:?}", plugin_id.name);
                slot_ids
                    .iter()
                    .map(|slot_id| cspace.revoke_tree(*slot_id))
                    .sum()
            }
        };
        // Slice 3 PluginCspace cleanup: drain the plugin's
        // own `PluginCspace` after the global revoke ran. A
        // builtin that minted into its per-plugin cspace and
        // granted a derived slot into the global cspace left
        // the local slot live (grant preserves the source).
        // The `RuinFn` signature stays untouched, so the
        // orchestrator has to drive the per-plugin reclaim
        // itself. `reclaim_plugin` is a no-op for plugins
        // that haven't migrated to per-plugin isolation.
        let local_revoked = factory.reclaim_plugin(plugin_id);
        let total_revoked = global_revoked + local_revoked;
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
