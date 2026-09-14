//! Personality / lifecycle / run — the top-level orchestrator.
//!
//! Phase 8 design: takes the three concrete builtins as separate
//! parameters and dispatches by plugin name. The trait-object
//! `Arc<dyn Builtin>` design hit a wall — the kernel's
//! `CapabilityFactory::mint<R>` is generic over `R`, and a trait
//! object can't dispatch into a generic call. For three
//! builtins the explicit dispatch is clearer than the erased
//! alternative (which would require kernel changes).
//!
//! Adding a fourth builtin means adding a fourth parameter and
//! a fourth match arm. That's the cost we pay for keeping the
//! typed `Capability<R>` invariant in the kernel.

use std::collections::HashMap;
use std::sync::Arc;

use crate::capability::enforce::quota::CapabilityBudget;
use crate::capability::enforce::space::CapabilitySpace;
use crate::personality::lifecycle::lifecycle_event::{LifecycleEvent, LifecycleEventBus};
use crate::capability::init::new_kernel;
use crate::core::contract::resource::Resource;
use crate::core::clock::clock::SystemClock;
use crate::core::contract::builtin::BuiltinManifest;
use crate::core::identity::ids::{PluginId, SlotId};
use crate::core::identity::kind::CapKind;
use crate::core::manifest::manifest::PluginManifest;
use crate::personality::composition::resolve::{resolve, ResolvedPlan};
use crate::personality::lifecycle::mint::CapabilityFactory;
use crate::personality::lifecycle::ruin::ruin_runtime_plugins;
use crate::personality::lifecycle::serve::spawn_http_bridge;

/// Top-level entry point. Boots the kernel, collects manifests
/// from the three concrete builtins, resolves, mints, serves
/// HTTP, and tears down on Ctrl-C.
pub async fn run<B1, B2, B3>(
    echo: Arc<B1>,
    reverse: Arc<B2>,
    database: Arc<B3>,
) -> Result<(), Box<dyn std::error::Error>>
where
    B1: BuiltinManifest + 'static,
    B1: EchoMint,
    B2: BuiltinManifest + 'static,
    B2: ReverseMint,
    B3: BuiltinManifest + 'static,
    B3: DatabaseMint,
{
    // 1. Setup — load manifests from the three builtins.
    let manifests: Vec<PluginManifest> = vec![
        echo.manifest(),
        reverse.manifest(),
        database.manifest(),
    ];
    println!("[boot] loaded {} builtin(s)", manifests.len());

    let ctx = cordis::Context::new();
    let clock: Arc<dyn crate::core::clock::clock::Clock> = Arc::new(SystemClock);
    let kernel = new_kernel(clock);
    let cspace: CapabilitySpace = kernel.space().clone();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    ctx.provide("capability_space", cspace.clone()).await?;
    ctx.provide("capability_factory", factory.clone()).await?;
    println!("[main] core services provided");

    // 2. Resolve.
    let plan: ResolvedPlan = resolve(&manifests).map_err(|e| format!("resolver: {e}"))?;
    println!("\n[resolver]\n{}", plan.render());

    // 3. Mint in resolved order — dispatch by plugin name.
    let minted = mint_three_builtins(&factory, &plan, &manifests, &echo, &reverse, &database).await?;

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
    ctx.stop().await;
    Ok(())
}

/// Marker traits — `run` requires the three builtins to provide
/// their typed `mint(...)` methods so we can dispatch by plugin
/// name.
pub trait EchoMint {
    fn echo_mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &crate::core::manifest::manifest::CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
    ) -> Result<SlotId, String>;
}

pub trait ReverseMint {
    fn reverse_mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &crate::core::manifest::manifest::CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
    ) -> Result<SlotId, String>;
}

pub trait DatabaseMint {
    fn database_mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &crate::core::manifest::manifest::CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
    ) -> Result<SlotId, String>;
}

async fn mint_three_builtins<B1, B2, B3>(
    factory: &CapabilityFactory,
    plan: &ResolvedPlan,
    manifests: &[PluginManifest],
    echo: &Arc<B1>,
    reverse: &Arc<B2>,
    database: &Arc<B3>,
) -> Result<HashMap<PluginId, Vec<SlotId>>, Box<dyn std::error::Error>>
where
    B1: BuiltinManifest + EchoMint,
    B2: BuiltinManifest + ReverseMint,
    B3: BuiltinManifest + DatabaseMint,
{
    use std::collections::BTreeMap;
    let by_id: BTreeMap<PluginId, &PluginManifest> = manifests
        .iter()
        .map(|m| (m.plugin.clone(), m))
        .collect();

    let mut minted: HashMap<PluginId, Vec<SlotId>> = HashMap::new();
    for plugin_id in &plan.mint_order {
        let Some(m) = by_id.get(plugin_id) else { continue };
        let mut plugin_slots = Vec::with_capacity(m.exposes.len());
        for decl in &m.exposes {
            let kind = if decl.streaming { CapKind::Stream } else { CapKind::Sync };
            let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
            let slot_id = match plugin_id.name.as_str() {
                "echo" => echo.echo_mint(factory, plugin_id, decl, kind, budget),
                "reverse" => reverse.reverse_mint(factory, plugin_id, decl, kind, budget),
                "database" => database.database_mint(factory, plugin_id, decl, kind, budget),
                other => { let msg = format!("unknown builtin {other:?}; add it to mint_three_builtins"); return Err(Box::new(std::io::Error::new(std::io::ErrorKind::NotFound, msg))); },
            }
            .map_err(|e| format!("mint {}::{}: {e}", plugin_id.name, decl.name))?;
            plugin_slots.push(slot_id);
        }
        minted.insert(plugin_id.clone(), plugin_slots);
    }
    Ok(minted)
}

#[allow(dead_code)]
fn _resource_bound<R: Resource>() {}
