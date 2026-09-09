//! Odyssey — seL4-style capability kernel, manifest-driven boot.
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
//!   Phase 1  Parse manifests
//!   Phase 2  Provide core services (capability_space, capability_factory, registry)
//!   Phase 3  Mint typed tokens, allocate slots, install + provide slots
//!   Phase 4  Validate graph — fail-fast on missing providers
//!   Phase 5  Start plugins — cordis resolves slot inject declarations
//!   Phase 6  Demo harness — invoke via typed slots
//!   Phase 7  HTTP bridge — enumerates via CSpace

use std::collections::HashSet;
use std::io::Write as _;
use std::sync::Arc;

use crate::capability::{Capability, CapabilityBudget, CapabilitySpace, CapKind, Slot};
use crate::host::factory::CapabilityFactory;
use crate::host::http_bridge::serve;
use crate::host::manifest::{CapabilityDecl, PluginId, PluginManifest};
use crate::plugins::{
    echo::{EchoResource, echo_plugin},
    echo_chain::{EchoChainResource, echo_chain_plugin},
    generator::{GeneratorResource, generator_plugin},
    reverse::{ReverseResource, reverse_plugin},
    sandbox::{SandboxResource, sandbox_plugin},
    slow::{SlowResource, slow_plugin},
    stream_echo::{StreamEchoResource, stream_echo_plugin},
};
use crate::host::registry::Registry;
use serde_json::json;

const MANIFEST_DIR: &str = "src/plugins";

// ---------------------------------------------------------------------------
// Streaming output helper
// ---------------------------------------------------------------------------

async fn print_stream(mut rx: tokio::sync::mpsc::Receiver<crate::capability::CapabilityChunk>) {
    while let Some(chunk) = rx.recv().await {
        match chunk {
            crate::capability::CapabilityChunk::Item(v) => {
                print!("{}", v.as_str().unwrap_or("?"));
                let _ = std::io::stdout().flush();
            }
            crate::capability::CapabilityChunk::Done => {
                println!("\n   [done]");
                break;
            }
        }
    }
}

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

/// 7-phase boot orchestration. Called by `main` in the top-level
/// `main.rs`; runs inside the `#[tokio::main]` runtime.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Phase 1: Parse manifests.
    let manifests = load_manifests(MANIFEST_DIR)?;
    println!("[manifest] loaded {} plugin(s):", manifests.len());
    for m in &manifests {
        println!(
            "  - {}@{}  {:?}  exposes={}",
            m.plugin.name,
            m.plugin.version,
            m.isolate,
            m.exposes.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(",")
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

    // Phase 3 + 4: Mint typed tokens, allocate slots, install + provide to cordis.
    println!("\n[mint] capability tokens:");

    let echo_slot_id = mint_and_provide::<EchoResource, _>(
        &ctx,
        &factory,
        &manifests,
        "echo",
        CapKind::Sync,
        "slot:echo",
        |_, _| crate::plugins::echo::handler(),
    )
    .await?;
    if let Some(id) = echo_slot_id {
        println!("  echo  →  slot={id}");
    }

    let reverse_slot_id = mint_and_provide::<ReverseResource, _>(
        &ctx,
        &factory,
        &manifests,
        "reverse",
        CapKind::Sync,
        "slot:reverse",
        |_, _| crate::plugins::reverse::handler(),
    )
    .await?;
    if let Some(id) = reverse_slot_id {
        println!("  reverse  →  slot={id}");
    }

    let slow_slot_id = mint_and_provide::<SlowResource, _>(
        &ctx,
        &factory,
        &manifests,
        "slow",
        CapKind::Sync,
        "slot:slow",
        |_, _| crate::plugins::slow::handler(),
    )
    .await?;
    if let Some(id) = slow_slot_id {
        println!("  slow  →  slot={id}");
    }

    let sandbox_slot_id = mint_and_provide::<SandboxResource, _>(
        &ctx,
        &factory,
        &manifests,
        "sandbox",
        CapKind::Sync,
        "slot:exec",
        |_, _| crate::plugins::sandbox::handler(),
    )
    .await?;
    if let Some(id) = sandbox_slot_id {
        println!("  exec  →  slot={id}");
    }

    // Echo-chain — depends on the typed Capability<EchoResource>.
    if let (Some(echo_id), Some(m)) = (
        echo_slot_id,
        manifests.iter().find(|m| m.plugin.name == "echo-chain"),
    ) {
        let decl = m.exposes.first().unwrap();
        let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
        let echo_cap = cspace
            .lookup_typed::<EchoResource>(echo_id)
            .ok_or_else(|| "echo-chain: typed echo capability missing")?;
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

    let stream_echo_slot_id = mint_and_provide::<StreamEchoResource, _>(
        &ctx,
        &factory,
        &manifests,
        "stream_echo",
        CapKind::Stream,
        "slot:stream_echo",
        |_, _| crate::plugins::stream_echo::handler(),
    )
    .await?;
    if let Some(id) = stream_echo_slot_id {
        println!("  stream_echo  →  slot={id}");
    }

    let gen_slot_id = mint_and_provide::<GeneratorResource, _>(
        &ctx,
        &factory,
        &manifests,
        "generator",
        CapKind::Stream,
        "slot:generate",
        |_, _| crate::plugins::generator::handler(),
    )
    .await?;
    if let Some(id) = gen_slot_id {
        println!("  generate  →  slot={id}");
    }

    // Track which capabilities ended up installed.
    let provided_caps: HashSet<String> = manifests
        .iter()
        .filter(|m| !matches!(m.plugin.name.as_str(), "echo-cdylib" | "echo-wasm"))
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

    // Phase 6: Demo — typed invocation via Slot.
    println!("\n[demo] capability invocations:");

    if let Some(echo_slot) = ctx
        .require::<Slot<EchoResource>>("slot:echo")
        .ok()
    {
        match echo_slot.invoke(json!({"message": "hello", "n": 42})) {
            Ok(v) => println!("  echo: {v}"),
            Err(e) => println!("  echo error: {e}"),
        }
    }
    if let Some(rev_slot) = ctx.require::<Slot<ReverseResource>>("slot:reverse").ok() {
        match rev_slot.invoke(json!("pipeline")) {
            Ok(v) => println!("  reverse: {v}"),
            Err(e) => println!("  reverse error: {e}"),
        }
    }
    if let Some(stream_slot) = ctx
        .require::<Slot<StreamEchoResource>>("slot:stream_echo")
        .ok()
    {
        match stream_slot.open(json!("hello world from stream_echo")) {
            Ok(rx) => {
                println!("  stream_echo:");
                print!("   ");
                print_stream(rx).await;
            }
            Err(e) => println!("  stream_echo error: {e}"),
        }
    }
    if let Some(slow_slot) = ctx.require::<Slot<SlowResource>>("slot:slow").ok() {
        println!(
            "\n  slow slot (timeout={}ms, will timeout):",
            slow_slot.meta().map(|m| m.timeout_ms).unwrap_or(0)
        );
        match slow_slot.invoke(json!({"hi": "limiter"})) {
            Ok(v) => println!("  [unexpected success] {v}"),
            Err(e) => println!("  [expected timeout] {e}"),
        }
    }

    // Factory mint snapshots.
    println!("\n[budget] minted token snapshots:");
    for snap in factory.snapshots() {
        println!(
            "  {}@{}: id={} timeout={}ms",
            snap.plugin.name, snap.plugin.version, snap.id, snap.timeout_ms
        );
    }

    // Pipeline — hold typed Capability<R> in stages.
    println!("\n[pipeline] token-based composition:");
    if let (Some(rev_slot), Some(echo_slot)) = (
        ctx.require::<Slot<ReverseResource>>("slot:reverse").ok(),
        ctx.require::<Slot<EchoResource>>("slot:echo").ok(),
    ) {
        let rev_cap: Arc<Capability<ReverseResource>> = rev_slot
            .capability()
            .ok_or("rev slot empty")?;
        let echo_cap: Arc<Capability<EchoResource>> = echo_slot
            .capability()
            .ok_or("echo slot empty")?;
        let stages = vec![
            crate::host::pipeline::SyncStage::new(rev_cap)?,
            crate::host::pipeline::SyncStage::new(echo_cap)?,
        ];
        let p = crate::host::pipeline::Pipeline::new(stages);
        match p.run(json!("hello")) {
            Ok(v) => println!("  reverse(echo(\"hello\")) = {v}"),
            Err(e) => println!("  [pipeline error] {e}"),
        }
    }

    // Generator streaming.
    if let Some(gen_slot) = ctx
        .require::<Slot<GeneratorResource>>("slot:generate")
        .ok()
    {
        match gen_slot.open(json!("hello there")) {
            Ok(rx) => {
                println!("  generate streaming:");
                print!("   ");
                print_stream(rx).await;
            }
            Err(e) => println!("  generate error: {e}"),
        }
    }

    // Sandbox via slot.
    println!("\n[sandbox] exec via slot (fuel=1_000_000):");
    if let Some(sandbox_slot) = ctx.require::<Slot<SandboxResource>>("slot:exec").ok() {
        match sandbox_slot.invoke(json!({
            "path": "src/plugins/sandbox/sandbox_programs/hello.wat",
            "fuel": 1_000_000
        })) {
            Ok(v) => println!("  {}", serde_json::to_string_pretty(&v).unwrap()),
            Err(e) => println!("  error: {e}"),
        }
    }

    // Demonstrate revocation: revoke echo's slot, then the plugin's lookup fails.
    if let Some(echo_slot_id) = echo_slot_id {
        println!("\n[revoke] clearing echo slot={echo_slot_id}...");
        cspace.revoke(echo_slot_id);
        if let Some(echo_slot) = ctx.require::<Slot<EchoResource>>("slot:echo").ok() {
            match echo_slot.invoke(json!({"after": "revoke"})) {
                Ok(v) => println!("  [unexpected] {v}"),
                Err(e) => println!("  [expected after revoke] {e}"),
            }
        }
    }

    // Demonstrate the four capability operations: grant / transfer /
    // restrict / revoke. Operates on the typed `Slot<R>` references
    // already in scope.
    println!("\n[ops] grant / transfer / restrict / revoke:");

    if let (Some(echo_id), Some(slow_id)) = (echo_slot_id, slow_slot_id) {
        // Re-mint echo for the demo since the slot above was just revoked.
        let echo_remint_id = factory.mint::<EchoResource>(
            CapKind::Sync,
            manifests.iter().find(|m| m.plugin.name == "echo").unwrap().exposes.first().unwrap(),
            &manifests.iter().find(|m| m.plugin.name == "echo").unwrap().plugin,
            CapabilityBudget::new(5000),
            crate::plugins::echo::handler(),
        );
        let _ = echo_id;
        let echo_slot = Slot::<EchoResource>::new(cspace.clone(), echo_remint_id);

        // 1) Grant: derive a new slot "echo_lite" with reduced timeout.
        let lite_rights = crate::capability::CapabilityRights::root(100);
        let lite_id = echo_slot.grant(lite_rights, "echo_lite".to_string())?;
        let lite_slot = Slot::<EchoResource>::new(cspace.clone(), lite_id);
        println!("  grant:    slot={lite_id} timeout=100ms (source preserved)");
        match echo_slot.invoke(json!({"via": "source"})) {
            Ok(_) => println!("    source echo still works"),
            Err(e) => println!("    source echo error: {e}"),
        }
        let lite_cap = lite_slot.capability().expect("lite slot populated");
        println!(
            "    lite cap timeout_ms={} ops={:?} id={}",
            lite_cap.rights().timeout_ms,
            lite_cap.operations(),
            lite_cap.id()
        );

        // 2) Restrict: same operation semantically, different intent.
        let strict_rights = crate::capability::CapabilityRights::root(50);
        let strict_id = echo_slot.restrict(strict_rights, "echo_strict".to_string())?;
        println!("  restrict: slot={strict_id} timeout=50ms");

        // 3) Transfer: move slow to a new slot "slow_moved" with new
        //    timeout. Source slot is cleared.
        //
        //    The slow slot was minted from slow.toml with
        //    `timeout_ms = 50`, so the transferred slot must use a
        //    subset of that budget (real attenuation, enforced by
        //    Phase 1's restrict/grant/transfer). 25ms is a valid
        //    subset.
        let slow_slot = Slot::<SlowResource>::new(cspace.clone(), slow_id);
        let moved_id = slow_slot.transfer(crate::capability::CapabilityRights::root(25))?;
        println!("  transfer: slot={moved_id} name=slow (source cleared)");
        match slow_slot.invoke(json!({})) {
            Ok(_) => println!("    [unexpected] source still works"),
            Err(e) => println!("    [expected] source empty: {e}"),
        }
        let moved_slot = Slot::<SlowResource>::new(cspace.clone(), moved_id);
        match moved_slot.invoke(json!({})) {
            Ok(_) => println!("    [unexpected] moved works (handler is 200ms, budget 1000ms)"),
            Err(e) => println!("    moved slot invoke: {e}"),
        }

        // 4) Revoke: drop the new strict slot.
        let _ = lite_slot.revoke();
        let _ = Slot::<EchoResource>::new(cspace.clone(), strict_id).revoke();
        println!("  revoke:   lite + strict slots cleared");
    }

    // Phase 7: HTTP bridge.
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

fn load_manifests(dir: &str) -> Result<Vec<PluginManifest>, Box<dyn std::error::Error>> {
    fn walk(p: &std::path::Path, out: &mut Vec<PluginManifest>) -> Result<(), Box<dyn std::error::Error>> {
        for entry in std::fs::read_dir(p)? {
            let entry = entry?;
            let path = entry.path();
            let ft = entry.file_type()?;
            if ft.is_dir() {
                walk(&path, out)?;
            } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                let m = PluginManifest::from_path(&path)
                    .map_err(|e| format!("{}: {e}", path.display()))?;
                out.push(m);
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(std::path::Path::new(dir), &mut out)?;
    out.sort_by(|a, b| a.plugin.name.cmp(&b.plugin.name));
    Ok(out)
}