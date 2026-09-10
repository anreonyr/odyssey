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
//!   Phase 3  Mint typed tokens + provide slots to cordis
//!   Phase 4  Validate the dependency graph (fail-fast on missing
//!            providers)
//!   Phase 5  Start plugins
//!   Phase 6  Bring up the HTTP bridge and wait for Ctrl-C
//!
//! Phase 6 in earlier versions exercised the caps with a demo
//! transcript; that has moved to `tests/{alpha,beta,gamma,delta}/`,
//! so the boot here stops at "kernel + plugins running".

use std::collections::HashSet;
use std::sync::Arc;

use crate::capability::{CapabilityBudget, CapabilitySpace, CapKind, Slot};
use crate::host::factory::CapabilityFactory;
use crate::host::http_bridge::serve;
use crate::host::manifest::{CapabilityDecl, PluginId, PluginManifest};
use crate::host::registry::Registry;
use crate::plugins::{
    echo::{echo_plugin, EchoResource},
    echo_chain::{echo_chain_plugin, EchoChainResource},
    generator::{generator_plugin, GeneratorResource},
    reverse::{reverse_plugin, ReverseResource},
    sandbox::{sandbox_plugin, SandboxResource},
    slow::{slow_plugin, SlowResource},
    stream_echo::{stream_echo_plugin, StreamEchoResource},
};

const MANIFEST_DIR: &str = "src/plugins";

// ---------------------------------------------------------------------------
// Mint helpers
// ---------------------------------------------------------------------------

/// Mint a token at a fresh slot, install into the CSpace, and provide
/// a `Slot<R>` handle to cordis. Returns the slot id.
async fn mint_and_provide<R, F>(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    manifests: &[PluginManifest],
    plugin_name: &str,
    kind: CapKind,
    slot_key: &str,
    handler_for: F,
) -> Result<Option<crate::capability::SlotId>, Box<dyn std::error::Error>>
where
    R: crate::capability::Resource + 'static,
    F: FnOnce(&CapabilityDecl, &PluginId) -> Arc<R>,
{
    let Some(m) = manifests.iter().find(|m| m.plugin.name == plugin_name) else {
        return Ok(None);
    };
    let decl = m.exposes.first().ok_or_else(|| {
        format!("{plugin_name}: manifest must expose at least one capability")
    })?;
    let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
    let handler = handler_for(decl, &m.plugin);
    let slot_id = factory.mint::<R>(kind, decl, &m.plugin, budget, handler);
    let slot = Slot::<R>::new(factory.space().clone(), slot_id);
    ctx.provide(slot_key, slot).await
        .map_err(|e| format!("provide {slot_key}: {e}"))?;
    Ok(Some(slot_id))
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Phase 1: Parse manifests.
    let LoadedManifests { ready, deferred } = load_manifests(MANIFEST_DIR)?;
    println!("[manifest] loaded {} plugin(s):", ready.len() + deferred.len());
    for m in &ready {
        println!(
            "  - {}@{}  {:?}  exposes={}",
            m.plugin.name,
            m.plugin.version,
            m.isolate,
            m.exposes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(",")
        );
    }
    for m in &deferred {
        println!(
            "  - {}@{}  {:?}  [skip: {}]",
            m.plugin.name,
            m.plugin.version,
            m.isolate,
            deferral_reason(&m.isolate),
        );
    }
    let manifests = ready;

    // Phase 2: Provide core services.
    let ctx = cordis::Context::new();
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace.clone());
    let registry = Arc::new(Registry::default());

    ctx.provide("capability_space", cspace.clone()).await?;
    ctx.provide("capability_factory", factory.clone()).await?;
    ctx.provide("registry", registry.clone()).await?;
    println!("[main] core services provided");

    // Phase 3: Mint typed tokens + provide slots to cordis.
    println!("\n[mint] capability tokens:");

    let echo_slot_id = mint_and_provide::<EchoResource, _>(
        &ctx, &factory, &manifests, "echo", CapKind::Sync, "slot:echo",
        |_, _| crate::plugins::echo::handler(),
    ).await?;
    let _reverse_slot_id = mint_and_provide::<ReverseResource, _>(
        &ctx, &factory, &manifests, "reverse", CapKind::Sync, "slot:reverse",
        |_, _| crate::plugins::reverse::handler(),
    ).await?;
    let _slow_slot_id = mint_and_provide::<SlowResource, _>(
        &ctx, &factory, &manifests, "slow", CapKind::Sync, "slot:slow",
        |_, _| crate::plugins::slow::handler(),
    ).await?;
    let _sandbox_slot_id = mint_and_provide::<SandboxResource, _>(
        &ctx, &factory, &manifests, "sandbox", CapKind::Sync, "slot:exec",
        |_, _| crate::plugins::sandbox::handler(),
    ).await?;

    // Echo-chain — depends on the typed Capability<EchoResource>.
    if let (Some(echo_id), Some(m)) = (
        echo_slot_id,
        manifests.iter().find(|m| m.plugin.name == "echo-chain"),
    ) {
        let decl = m.exposes.first().unwrap();
        let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
        let echo_cap = cspace
            .lookup_typed::<EchoResource>(echo_id)
            .expect("echo cap present at the slot we just minted");
        let slot_id = factory.mint::<EchoChainResource>(
            CapKind::Sync,
            decl,
            &m.plugin,
            budget,
            crate::plugins::echo_chain::handler(echo_cap),
        );
        let slot = Slot::<EchoChainResource>::new(cspace.clone(), slot_id);
        ctx.provide("slot:echo_chain", slot).await?;
        println!("  echo_chain  →  slot={slot_id}");
    }

    mint_and_provide::<StreamEchoResource, _>(
        &ctx, &factory, &manifests, "stream_echo", CapKind::Stream, "slot:stream_echo",
        |_, _| crate::plugins::stream_echo::handler(),
    ).await?;
    mint_and_provide::<GeneratorResource, _>(
        &ctx, &factory, &manifests, "generator", CapKind::Stream, "slot:generate",
        |_, _| crate::plugins::generator::handler(),
    ).await?;

    // Track which capabilities ended up installed.
    let provided_caps: HashSet<String> = manifests
        .iter()
        .flat_map(|m| m.exposes.iter().map(|c| c.name.clone()))
        .collect();

    // Phase 4: Validate graph.
    println!("\n[graph] validating capability dependencies:");
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
                missing.push((m.plugin.name.clone(), dep.plugin.clone(), dep.capability.clone()));
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

    // Phase 5: Start plugins.
    println!("\n[plugins] activating:");
    let plugin_list: Vec<(&str, Arc<dyn cordis::Plugin>)> = vec![
        ("echo", echo_plugin()),
        ("reverse", reverse_plugin()),
        ("slow", slow_plugin()),
        ("stream_echo", stream_echo_plugin()),
        ("generator", generator_plugin()),
        ("echo-chain", echo_chain_plugin()),
        ("sandbox", sandbox_plugin()),
    ];
    let active_plugins: Vec<_> = plugin_list
        .into_iter()
        .filter(|(name, _)| {
            manifests.iter().any(|m| {
                m.plugin.name == *name
                    && m.exposes.iter().all(|c| provided_caps.contains(&c.name))
            })
        })
        .collect();
    let plugin_fibers: Vec<_> = active_plugins
        .iter()
        .map(|(name, plugin)| (*name, ctx.plugin(plugin.clone(), None)))
        .collect();

    for (name, handle) in &plugin_fibers {
        match handle.join().await {
            Ok(()) => println!("  ✓ {name} activated"),
            Err(e) => eprintln!("  ✗ {name} failed: {e}"),
        }
    }

    // Phase 6: HTTP bridge + wait.
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
    ctx.stop().await;

    Ok(())
}

/// Manifests ready for the runtime, plus manifests that parsed
/// cleanly but are not currently wired in.
struct LoadedManifests {
    ready: Vec<PluginManifest>,
    deferred: Vec<PluginManifest>,
}

/// `true` when the runtime knows how to actually instantiate this
/// manifest. Today only `InProc` plugins are wired; `Wasm` and
/// `Subprocess` are deferred.
fn is_runtime_supported(m: &PluginManifest) -> bool {
    matches!(m.isolate, crate::host::manifest::IsolationMode::InProc)
}

fn deferral_reason(iso: &crate::host::manifest::IsolationMode) -> &'static str {
    match iso {
        crate::host::manifest::IsolationMode::InProc => "in-proc (no skip needed)",
        crate::host::manifest::IsolationMode::Wasm { .. } => "wasm loader not wired",
        crate::host::manifest::IsolationMode::Subprocess { .. } => {
            "subprocess transport not wired"
        }
    }
}

fn load_manifests(dir: &str) -> Result<LoadedManifests, Box<dyn std::error::Error>> {
    fn walk(
        p: &std::path::Path,
        ready: &mut Vec<PluginManifest>,
        deferred: &mut Vec<PluginManifest>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for entry in std::fs::read_dir(p)? {
            let entry = entry?;
            let path = entry.path();
            let ft = entry.file_type()?;
            if ft.is_dir() {
                walk(&path, ready, deferred)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let m = PluginManifest::from_path(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                if is_runtime_supported(&m) {
                    ready.push(m);
                } else {
                    deferred.push(m);
                }
            }
        }
        Ok(())
    }
    let mut ready = Vec::new();
    let mut deferred = Vec::new();
    walk(std::path::Path::new(dir), &mut ready, &mut deferred)?;
    ready.sort_by(|a, b| a.plugin.name.cmp(&b.plugin.name));
    deferred.sort_by(|a, b| a.plugin.name.cmp(&b.plugin.name));
    Ok(LoadedManifests { ready, deferred })
}