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
//! MintFn, RuinFn)` helper; the third tuple element is a hook
//! the orchestrator fires before `cspace.revoke_tree` per
//! plugin so the hook sees live slots (a future persistence
//! builtin flushing to disk, or a streaming builtin draining
//! its producer task, would override the default). Today every
//! builtin's `RuinFn` is `default_ruin` (just the
//! `cspace.revoke_tree` loop); the slot is reserved for
//! future teardown work.

use std::collections::HashMap;
use std::sync::Arc;

use crate::capability::enforce::quota::CapabilityBudget;
use crate::capability::enforce::space::CapabilitySpace;
use crate::core::clock::clock::SystemClock;
use crate::core::identity::ids::{PluginId, SlotId};
use crate::core::identity::kind::CapKind;
use crate::core::manifest::manifest::{CapabilityDecl, PluginManifest};
use crate::personality::composition::resolve::{resolve, ResolvedPlan};
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
pub type MintFn = fn(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    decl: &CapabilityDecl,
    kind: CapKind,
    budget: CapabilityBudget,
) -> SlotId;

/// Per-plugin teardown hook. Called by the orchestrator
/// *before* `cspace.revoke_tree` per slot so the hook sees
/// live slots (a future builtin with custom teardown — e.g.
/// flushing state, draining a producer task — needs to read
/// its own slot's state before the cspace evicts it).
///
/// Sync today (no current builtin needs to await on
/// teardown). If a future builtin needs async drain,
/// promote the return to `impl Future<Output = Result<(), String>>`
/// and add `.await` at the call site in `run` — the
/// signature change is local to this file.
pub type RuinFn = fn(cspace: &CapabilitySpace, slot_ids: &[SlotId]) -> Result<(), String>;

/// Default teardown — the same loop `ruin_runtime_plugins`
/// did inline before Phase 11. Exposed so every builtin's
/// `register()` can reference the same symbol instead of
/// duplicating the loop body.
pub fn default_ruin(cspace: &CapabilitySpace, slot_ids: &[SlotId]) -> Result<(), String> {
    for id in slot_ids {
        cspace.revoke_tree(*id);
    }
    Ok(())
}

/// Top-level entry point. Boots the kernel, resolves the
/// provided `(manifest, mint_fn, ruin_fn)` triples, mints each
/// capability via the matching `mint_fn`, serves HTTP, and
/// tears down on Ctrl-C.
pub async fn run(
    plugins: &[(PluginManifest, MintFn, RuinFn)],
) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup — split manifests from dispatch fns for the
    // resolver.
    let manifests: Vec<PluginManifest> =
        plugins.iter().map(|(m, _, _)| m.clone()).collect();
    println!("[boot] loaded {} builtin(s)", manifests.len());

    let cspace: CapabilitySpace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    // 2. Resolve.
    let plan: ResolvedPlan = resolve(&manifests).map_err(|e| format!("resolver: {e}"))?;
    println!("\n[resolver]\n{}", plan.render());

    // 3. Mint in resolved order — dispatch by plugin name through
    // the registry.
    let minted = mint_from_registry(&factory, &plan, &manifests, plugins).await?;

    // 4. Serve.
    let server_handle =
        spawn_http_bridge("127.0.0.1:3030".parse().unwrap(), cspace.clone());
    eprintln!("\n[main] HTTP bridge up — open http://127.0.0.1:3030/");
    eprintln!("[main] press Ctrl-C to stop");
    let _ = server_handle.await;

    // 5. Teardown — per-plugin `RuinFn` (default or custom)
    // fires before `cspace.revoke_tree` per slot so the hook
    // sees live slots. Catch_unwind protects the orchestrator
    // from a panicking builtin; the cspace revoke still runs
    // afterwards so we don't leak slots.
    let lifecycle = LifecycleEventBus::new();
    let _ = lifecycle.publish(LifecycleEvent::ShutdownStarted);
    ruin_via_registry(&cspace, &plan, &minted, plugins);
    let _ = lifecycle.publish(LifecycleEvent::ShutdownCompleted {
        remaining_slots: cspace.len(),
    });
    Ok(())
}

async fn mint_from_registry(
    factory: &CapabilityFactory,
    plan: &ResolvedPlan,
    manifests: &[PluginManifest],
    plugins: &[(PluginManifest, MintFn, RuinFn)],
) -> Result<HashMap<PluginId, Vec<SlotId>>, Box<dyn std::error::Error>> {
    use std::collections::BTreeMap;
    let by_id: BTreeMap<PluginId, &PluginManifest> = manifests
        .iter()
        .map(|m| (m.plugin.clone(), m))
        .collect();

    // Index the registry by plugin name for O(log n) lookup
    // against `plan.mint_order`. Length-check catches
    // duplicate plugin names — a contributor error rather
    // than a silent drop.
    let by_name: BTreeMap<&str, (MintFn, RuinFn)> = plugins
        .iter()
        .map(|(m, mint_fn, ruin_fn)| (m.plugin.name.as_str(), (*mint_fn, *ruin_fn)))
        .collect();
    if by_name.len() != plugins.len() {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "duplicate plugin names in registry: {} entries -> {} unique",
                plugins.len(),
                by_name.len()
            ),
        )));
    }

    let mut minted: HashMap<PluginId, Vec<SlotId>> = HashMap::new();
    for plugin_id in &plan.mint_order {
        let Some(m) = by_id.get(plugin_id) else { continue };
        let Some(&(mint_fn, _ruin_fn)) = by_name.get(plugin_id.name.as_str()) else {
            let msg = format!(
                "no mint fn registered for plugin {:?}; add it to the plugins list",
                plugin_id.name
            );
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::NotFound, msg)));
        };
        let mut plugin_slots = Vec::with_capacity(m.exposes.len());
        for decl in &m.exposes {
            let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
            let slot_id = mint_fn(factory, plugin_id, decl, decl.kind, budget);
            plugin_slots.push(slot_id);
        }
        minted.insert(plugin_id.clone(), plugin_slots);
    }
    Ok(minted)
}

fn ruin_via_registry(
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    minted: &HashMap<PluginId, Vec<SlotId>>,
    plugins: &[(PluginManifest, MintFn, RuinFn)],
) {
    use std::collections::BTreeMap;
    let by_name: BTreeMap<&str, RuinFn> = plugins
        .iter()
        .map(|(m, _, ruin_fn)| (m.plugin.name.as_str(), *ruin_fn))
        .collect();

    println!("\n[shutdown] tearing down runtime plugins (reverse mint order):");
    for plugin_id in plan.mint_order.iter().rev() {
        let Some(slot_ids) = minted.get(plugin_id) else { continue };
        let Some(&ruin_fn) = by_name.get(plugin_id.name.as_str()) else {
            eprintln!(
                "[shutdown] {} has minted slots but no registered RuinFn; \
                 falling back to default_ruin (this is a contributor error — \
                 every register() should produce a 3-tuple)",
                plugin_id.name
            );
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                default_ruin(cspace, slot_ids)
            }));
            continue;
        };
        // Fire the per-plugin RuinFn with catch_unwind
        // protection. A panicking hook is a contributor bug;
        // log it and continue so the cspace revoke below still
        // runs and we don't leak slots.
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| ruin_fn(cspace, slot_ids)));
        if let Err(payload) = result {
            eprintln!("[shutdown] {} RuinFn panicked: {:?}", plugin_id.name, payload);
        }
        // cspace revoke still runs even if the hook panicked —
        // mandatory teardown, not best-effort.
        let mut total_revoked = 0usize;
        for slot_id in slot_ids {
            total_revoked += cspace.revoke_tree(*slot_id);
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
