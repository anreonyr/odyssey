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
//! a `MintFn` function-pointer registry keyed by plugin name:
//! the example builds a `&[(PluginManifest, MintFn)]` list at
//! boot, and the orchestrator dispatches by `mint_registry
//! .get(plugin_id.name.as_str())`. Builtins expose a colocated
//! `fn register() -> (PluginManifest, MintFn)` helper so a
//! new builtin can't forget either half.

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
use crate::personality::lifecycle::ruin::ruin_runtime_plugins;
use crate::personality::lifecycle::serve::spawn_http_bridge;

/// Typed mint entry point — the kernel's
/// `CapabilityFactory::mint<R>` is generic over `R` so this is
/// a free function pointer (not a trait object). Each builtin
/// exposes a non-capturing closure of this shape via its
/// `fn register() -> (PluginManifest, MintFn)` helper. The
/// orchestrator dispatches by `plugin_id.name.as_str()`
/// against a registry of these.
pub type MintFn = fn(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    decl: &CapabilityDecl,
    kind: CapKind,
    budget: CapabilityBudget,
) -> SlotId;

/// Top-level entry point. Boots the kernel, resolves the
/// provided `(manifest, mint_fn)` pairs, mints each capability
/// via the matching `mint_fn`, serves HTTP, and tears down on
/// Ctrl-C.
pub async fn run(plugins: &[(PluginManifest, MintFn)]) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup — split manifests from mint fns for the resolver.
    let manifests: Vec<PluginManifest> = plugins.iter().map(|(m, _)| m.clone()).collect();
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

    // 5. Teardown — lifecycle events go to the personality bus.
    let lifecycle = LifecycleEventBus::new();
    let _ = lifecycle.publish(LifecycleEvent::ShutdownStarted);
    ruin_runtime_plugins(&cspace, &lifecycle, &plan, &minted).await;
    let _ = lifecycle.publish(LifecycleEvent::ShutdownCompleted {
        remaining_slots: cspace.len(),
    });
    Ok(())
}

async fn mint_from_registry(
    factory: &CapabilityFactory,
    plan: &ResolvedPlan,
    manifests: &[PluginManifest],
    plugins: &[(PluginManifest, MintFn)],
) -> Result<HashMap<PluginId, Vec<SlotId>>, Box<dyn std::error::Error>> {
    use std::collections::BTreeMap;
    let by_id: BTreeMap<PluginId, &PluginManifest> = manifests
        .iter()
        .map(|m| (m.plugin.clone(), m))
        .collect();

    // Index the registry by plugin name for O(log n) lookup
    // against `plan.mint_order`.
    let by_name: BTreeMap<&str, MintFn> = plugins
        .iter()
        .map(|(m, mint_fn)| (m.plugin.name.as_str(), *mint_fn))
        .collect();

    let mut minted: HashMap<PluginId, Vec<SlotId>> = HashMap::new();
    for plugin_id in &plan.mint_order {
        let Some(m) = by_id.get(plugin_id) else { continue };
        let Some(&mint_fn) = by_name.get(plugin_id.name.as_str()) else {
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
