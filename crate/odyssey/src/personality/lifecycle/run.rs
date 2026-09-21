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
use crate::core::manifest::BundleId;
use crate::core::manifest::manifest::{CapabilityDecl, PluginManifest};
use crate::personality::composition::resolve::{ResolvedBinding, ResolvedPlan, resolve};
use crate::personality::lifecycle::lifecycle_event::{LifecycleEvent, LifecycleEventBus};
use crate::personality::lifecycle::mint::{
    CapabilityFactory, MintError, TypedBinding, TypedBindings,
};

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
/// is empty for a plugin that declares no dependencies.
///
/// DI Phase 21: `typed_bindings` is the pre-resolved typed slot
/// table the provision step produces. Each entry pairs the
/// consumer's local handle with a `SlotId` in the *global*
/// cspace (where the provider's `grant_to` already installed
/// the typed cap). The consumer's `Slot<R>` is constructed
/// with `Slot::new(factory.space().clone(), typed_binding.slot_id)`;
/// `Slot::capability()` does the RTTI downcast at use time.
///
/// DI Phase 21: returns `Result<SlotId, MintError>` instead of
/// `SlotId`. Three failure modes are typed:
/// - `HandlerReturned` — a future builtin returns an error
///   from its resource constructor (no current builtin does;
///   the variant is here so error-aware mints have a path).
/// - `GrantFailed` — the cross-cspace `grant_to` from
///   `pc.inner()` to `factory.space()` failed (e.g.
///   `AttenuationViolation`, `SlotEmpty`).
/// - `Panicked` — surfaced by the orchestrator's `catch_unwind`
///   wrapper around the `MintFn` call (mirroring how the
///   `RuinFn` path handles panics).
pub type MintFn = fn(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    decl: &CapabilityDecl,
    kind: CapKind,
    budget: CapabilityBudget,
    bindings: &[ResolvedBinding],
    typed_bindings: &TypedBindings,
) -> Result<SlotId, MintError>;

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

/// Top-level entry point. Boots the kernel, resolves the
/// provided `(manifest, mint_fn, ruin_fn)` triples, mints each
/// capability via the matching `mint_fn`, blocks on Ctrl-C, and
/// tears down. The HTTP bridge (if any) is a regular plugin —
/// its mint reads `ODYSSEY_ADDR` / `ODYSSEY_NO_FRONTEND` /
/// `ODYSSEY_FRONTEND_DIST` from the environment. The
/// orchestrator does not name or know about any specific plugin.
pub async fn run(
    plugins: &[(PluginManifest, MintFn, RuinFn)],
) -> Result<(), Box<dyn std::error::Error>> {
    run_on(plugins).await
}

/// `run` with an explicit bridge address.
///
/// The address is a parameter rather than a constant so a test can
/// give its spawned server a port of its own. Without that, a test
/// that starts the example binary either fights whatever already
/// holds the fixed port, or — worse — silently connects to it and
/// asserts against a build that is not the one under test.
///
/// The HTTP bridge is now a regular plugin (`http_bridge`); the
/// plugin's mint reads `ODYSSEY_ADDR` / `ODYSSEY_NO_FRONTEND` /
/// `ODYSSEY_FRONTEND_DIST` from the environment. The orchestrator
/// no longer threads the address or the frontend dist — both are
/// the bridge plugin's concern, not the orchestrator's.
pub async fn run_on(
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

    // DI Phase 21: provision typed bindings between resolve and
    // mint. The provisioner pre-fetches each consumer's binding
    // `slot_id`s from the global cspace (where the provider's
    // `grant_to` installed them) so consumer `MintFn`s don't
    // have to call `slot_for_name` themselves.
    let typed_bindings = provision_dependencies(&factory, &plan);

    // 3. Mint in resolved order — dispatch by plugin name through
    // the registry. The bridge plugin's mint reads
    // `ODYSSEY_ADDR` / `ODYSSEY_NO_FRONTEND` /
    // `ODYSSEY_FRONTEND_DIST` from the environment (see
    // `example/back/src/bridge.rs`).
    let minted = mint_from_registry(&factory, &plan, &typed_bindings, plugins).await?;
    // 4. Wait for shutdown — the orchestrator blocks on Ctrl-C
    // and then triggers each plugin's `RuinFn` in reverse mint
    // order. The orchestrator doesn't know which plugins are
    // running; each plugin owns its own lifecycle (the bridge
    // plugin's Resource, for example, drops its server task on
    // `RuinFn`). The bridge prints `[http] listening` itself;
    // we print nothing here so the orchestrator stays
    // plugin-agnostic.
    eprintln!("[main] waiting for Ctrl-C");
    let _ = tokio::signal::ctrl_c().await;

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
    typed_bindings: &HashMap<PluginId, TypedBindings>,
    plugins: &[(PluginManifest, MintFn, RuinFn)],
) -> Result<HashMap<PluginId, Vec<SlotId>>, Box<dyn std::error::Error>> {
    let by_name = plugin_registry(plugins)?;

    let mut minted: HashMap<PluginId, Vec<SlotId>> = HashMap::new();
    for plugin_id in &plan.mint_order {
        let entry = &by_name[plugin_id.name.as_str()];
        let resolver_bindings: &[ResolvedBinding] = plan
            .bindings
            .get(plugin_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        // DI Phase 21: typed bindings are pre-fetched by the
        // provision step (see `provision_dependencies`). Plugins
        // with no `requires` see `TypedBindings::default()`.
        let empty = TypedBindings::default();
        let typed = typed_bindings.get(plugin_id).unwrap_or(&empty);
        let mut plugin_slots = Vec::with_capacity(entry.manifest.exposes.len());
        for decl in &entry.manifest.exposes {
            let budget = CapabilityBudget::new(entry.manifest.timeout_ms.unwrap_or(5000));
            // DI Phase 21: `catch_unwind` symmetric to the ruin
            // path. A panicking builtin becomes
            // `MintError::Panicked` (mirroring how the ruin
            // path handles panics at `RuinFn`). The builtin's
            // own `Result::Err` flows through unchanged.
            let mint_call = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                (entry.mint_fn)(
                    factory,
                    plugin_id,
                    decl,
                    decl.kind,
                    budget,
                    resolver_bindings,
                    typed,
                )
            }));
            let slot_id = match mint_call {
                Ok(Ok(slot)) => slot,
                Ok(Err(e)) => return Err(Box::new(e)),
                Err(payload) => {
                    return Err(Box::new(MintError::Panicked {
                        plugin: plugin_id.name.clone(),
                        payload,
                    }));
                }
            };
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
    // Collect per-plugin teardown results during iteration, then
    // print them grouped by bundle after iteration completes. The
    // underlying iteration is still reverse mint — preserved for
    // any future custom `RuinFn` whose order-of-revoke semantics
    // depend on dependencies being torn down consumers-first.
    // Grouping is purely a display affordance.
    //
    // `record` shape: (PluginId, total_revoked) — what to print.
    // Plugins with no minted slots (or whose `RuinFn` returned
    // 0 with no slots to revoke) are filtered out, matching the
    // pre-bundle behaviour where the per-plugin line only
    // printed when there was something to report.
    let mut record: Vec<(PluginId, usize)> = Vec::new();
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
            record.push((plugin_id.clone(), total_revoked));
        }
    }
    // Decide grouping: fall back to the pre-bundle flat format
    // when no plugin in the plan carries a bundle (mirrors the
    // logic in `ResolvedPlan::render`). Group order is determined
    // by the FIRST appearance of each bundle in `record` —
    // `record` is in reverse-mint order, so the "first" bundle
    // we encounter is the LAST one to be torn down (the last to
    // be torn down is conventionally the consumer that ran last
    // — e.g., `agent` bundle). The output reads top-to-bottom in
    // teardown order; bundle headings appear in the same order.
    let any_bundled = plan.bundles.values().any(|b| b.is_some());
    if !any_bundled {
        for (pid, total) in &record {
            println!(
                "  ✓ {}@{}  revoked {} slot(s)",
                pid.name, pid.version, total
            );
        }
    } else {
        let mut group_order: Vec<Option<BundleId>> = Vec::new();
        let mut group_index: std::collections::HashMap<Option<BundleId>, usize> =
            std::collections::HashMap::new();
        let mut groups: Vec<Vec<(PluginId, usize)>> = Vec::new();
        for (pid, total) in &record {
            let bundle = plan.bundles.get(pid).cloned().unwrap_or(None);
            let idx = match group_index.get(&bundle) {
                Some(&i) => i,
                None => {
                    group_index.insert(bundle.clone(), group_order.len());
                    group_order.push(bundle.clone());
                    groups.push(Vec::new());
                    group_order.len() - 1
                }
            };
            groups[idx].push((pid.clone(), *total));
        }
        for (bundle, members) in group_order.iter().zip(groups.iter()) {
            match bundle {
                Some(b) => println!("  [bundle {b}]"),
                None => println!("  [unbundled]"),
            }
            for (pid, total) in members {
                println!(
                    "    ✓ {}@{}  revoked {} slot(s)",
                    pid.name, pid.version, total
                );
            }
        }
    }
    let remaining = cspace.len();
    println!("[shutdown] cspace remaining slots: {remaining}");
}

// ---------------------------------------------------------------------------
// DI Phase 21: typed-binding provisioner
// ---------------------------------------------------------------------------

/// Pre-fetch each consumer's binding slot_ids from the global
/// cspace. Each provider's `grant_to` already installed the
/// typed `Arc<Capability<R>>` into the global cspace under the
/// provider's declared cap name; this step resolves the
/// `(handle, capability)` pair from each consumer's binding row
/// to that slot_id, so the consumer's `MintFn` doesn't have to
/// walk bindings or call `slot_for_name` itself.
///
/// The provisioner runs after `resolve()` (which has the binding
/// table) and before `mint_from_registry()` (which calls the
/// consumer's `MintFn`). Topological order is already established
/// by `plan.mint_order`, so the provider's cap is guaranteed to
/// be in the global cspace before any consumer reads it.
fn provision_dependencies(
    factory: &CapabilityFactory,
    plan: &ResolvedPlan,
) -> HashMap<PluginId, TypedBindings> {
    let mut out: HashMap<PluginId, TypedBindings> = HashMap::new();
    let space = factory.space();
    for plugin_id in &plan.mint_order {
        let Some(bindings) = plan.bindings.get(plugin_id) else {
            continue;
        };
        let mut typed = TypedBindings::default();
        for b in bindings {
            // Look up the provider's typed cap in the global
            // cspace. Every Slice 3 builtin grants to
            // `factory.space()` via `pc.inner().grant_to(...)`,
            // so the cap exists under the provider's declared
            // `cap_name` (matches `ResolvedBinding::capability`).
            let Some(slot_id) = space.slot_for_name(&b.capability) else {
                // Should be unreachable: `resolve()`'s
                // `Unprovided` check fires earlier if no
                // provider exposes the contract. If we land
                // here something has gone wrong in the
                // orchestrator pipeline; skip rather than
                // panic — the consumer's mint body still
                // sees an empty TypedBindings and any name
                // lookup fails loudly.
                continue;
            };
            typed.entries.push(TypedBinding {
                handle: b.handle.clone(),
                slot_id,
            });
        }
        out.insert(plugin_id.clone(), typed);
    }
    out
}
