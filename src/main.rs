//! Odyssey — seL4-style capability kernel, manifest-driven boot.
//!
//! Capability is generic over the resource type the plugin module declared:
//! `Capability<EchoResource>`, `Capability<ReverseResource>`, etc. The
//! type system enforces that a plugin only invokes capabilities it was
//! handed. Main holds typed tokens for the demo harness; cordis injects
//! pass them through to plugins; the shared `CapabilityService` registry
//! holds type-erased `Arc<dyn AnyCapability>` views for the HTTP bridge.

mod capability;
mod dispatcher;
mod http_bridge;
mod manifest;
mod pipeline;
mod plugins;
mod registry;

use std::collections::HashSet;
use std::io::Write as _;
use std::sync::Arc;

use capability::{
    AnyCapability, Capability, CapabilityBudget, CapabilityService, StreamKind, StreamResource,
    SyncKind, SyncResource,
};
use dispatcher::CapabilityFactory;
use http_bridge::serve;
use manifest::{CapabilityDecl, PluginId, PluginManifest};
use plugins::{
    echo::{EchoResource, echo_plugin},
    echo_chain::{EchoChainResource, echo_chain_plugin},
    generator::{GeneratorResource, generator_plugin},
    reverse::{ReverseResource, reverse_plugin},
    sandbox::{SandboxResource, sandbox_plugin},
    slow::{SlowResource, slow_plugin},
    stream_echo::{StreamEchoResource, stream_echo_plugin},
};
use registry::Registry;
use serde_json::json;

const MANIFEST_DIR: &str = "plugins";

// ---------------------------------------------------------------------------
// Streaming output helper
// ---------------------------------------------------------------------------

async fn print_stream(mut rx: tokio::sync::mpsc::Receiver<capability::CapabilityChunk>) {
    while let Some(chunk) = rx.recv().await {
        match chunk {
            capability::CapabilityChunk::Item(v) => {
                print!("{}", v.as_str().unwrap_or("?"));
                let _ = std::io::stdout().flush();
            }
            capability::CapabilityChunk::Done => {
                println!("\n   [done]");
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Mint helpers — pull handler from a plugin module, mint typed token.
// ---------------------------------------------------------------------------

fn mint_sync<R, F>(
    factory: &CapabilityFactory,
    manifests: &[PluginManifest],
    plugin_name: &str,
    handler_for: F,
) -> Result<Option<Arc<Capability<R, SyncKind>>>, Box<dyn std::error::Error>>
where
    R: SyncResource + 'static,
    F: FnOnce(&CapabilityDecl, &PluginId) -> Arc<R>,
{
    let Some(m) = manifests.iter().find(|m| m.plugin.name == plugin_name) else {
        return Ok(None);
    };
    let decl = m
        .exposes
        .first()
        .ok_or_else(|| format!("{plugin_name}: manifest must expose at least one capability"))?;
    let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
    let handler = handler_for(decl, &m.plugin);
    Ok(Some(factory.mint_sync::<R>(decl, &m.plugin, budget, handler)))
}

fn mint_stream<R, F>(
    factory: &CapabilityFactory,
    manifests: &[PluginManifest],
    plugin_name: &str,
    handler_for: F,
) -> Result<Option<Arc<Capability<R, StreamKind>>>, Box<dyn std::error::Error>>
where
    R: StreamResource + 'static,
    F: FnOnce(&CapabilityDecl, &PluginId) -> Arc<R>,
{
    let Some(m) = manifests.iter().find(|m| m.plugin.name == plugin_name) else {
        return Ok(None);
    };
    let decl = m
        .exposes
        .first()
        .ok_or_else(|| format!("{plugin_name}: manifest must expose at least one capability"))?;
    let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
    let handler = handler_for(decl, &m.plugin);
    Ok(Some(factory.mint_stream::<R>(decl, &m.plugin, budget, handler)))
}

// ---------------------------------------------------------------------------
// Boot
// ---------------------------------------------------------------------------

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
    let factory = CapabilityFactory::new();
    let cap_svc = Arc::new(CapabilityService::new());
    let registry = Arc::new(Registry::default());

    ctx.provide("capability_factory", factory.clone()).await?;
    ctx.provide("capability_service", (*cap_svc).clone()).await?;
    ctx.provide("registry", registry.clone()).await?;
    println!("[main] core services provided");

    // Phase 3 + 4: Mint typed tokens, provide to cordis, register in service.
    println!("\n[mint] capability tokens:");

    // Echo — sync, no deps.
    let echo_token = mint_sync::<EchoResource, _>(&factory, &manifests, "echo", |_, _| {
        plugins::echo::handler()
    })?;
    if let Some(t) = &echo_token {
        ctx.provide("cap:echo", (*t.as_ref()).clone()).await?;
        cap_svc.register(t.clone() as Arc<dyn AnyCapability>)?;
        println!("  echo  →  id={}  timeout={}ms", t.id(), t.meta().timeout_ms);
    }

    // Reverse — sync, no deps.
    let reverse_token = mint_sync::<ReverseResource, _>(&factory, &manifests, "reverse", |_, _| {
        plugins::reverse::handler()
    })?;
    if let Some(t) = &reverse_token {
        ctx.provide("cap:reverse", (*t.as_ref()).clone()).await?;
        cap_svc.register(t.clone() as Arc<dyn AnyCapability>)?;
        println!("  reverse  →  id={}  timeout={}ms", t.id(), t.meta().timeout_ms);
    }

    // Slow — sync, no deps.
    let slow_token = mint_sync::<SlowResource, _>(&factory, &manifests, "slow", |_, _| {
        plugins::slow::handler()
    })?;
    if let Some(t) = &slow_token {
        ctx.provide("cap:slow", (*t.as_ref()).clone()).await?;
        cap_svc.register(t.clone() as Arc<dyn AnyCapability>)?;
        println!("  slow  →  id={}  timeout={}ms", t.id(), t.meta().timeout_ms);
    }

    // Sandbox — sync, no deps.
    let sandbox_token = mint_sync::<SandboxResource, _>(&factory, &manifests, "sandbox", |_, _| {
        plugins::sandbox::handler()
    })?;
    if let Some(t) = &sandbox_token {
        ctx.provide("cap:exec", (*t.as_ref()).clone()).await?;
        cap_svc.register(t.clone() as Arc<dyn AnyCapability>)?;
        println!("  exec  →  id={}  timeout={}ms", t.id(), t.meta().timeout_ms);
    }

    // Echo-chain — sync, depends on echo.
    let chain_token =
        if let (Some(echo_t), Some(m)) = (
            echo_token.as_ref(),
            manifests.iter().find(|m| m.plugin.name == "echo-chain"),
        ) {
            let decl = m.exposes.first().unwrap();
            let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
            let t = factory.mint_sync::<EchoChainResource>(
                decl,
                &m.plugin,
                budget,
                plugins::echo_chain::handler(echo_t.clone()),
            );
            ctx.provide("cap:echo_chain", (*t.as_ref()).clone()).await?;
            cap_svc.register(t.clone() as Arc<dyn AnyCapability>)?;
            println!(
                "  echo_chain  →  id={}  timeout={}ms",
                t.id(),
                t.meta().timeout_ms
            );
            Some(t)
        } else {
            None
        };

    // Stream_echo — stream, no deps.
    let stream_echo_token =
        mint_stream::<StreamEchoResource, _>(&factory, &manifests, "stream_echo", |_, _| {
            plugins::stream_echo::handler()
        })?;
    if let Some(t) = &stream_echo_token {
        ctx.provide("cap:stream_echo", (*t.as_ref()).clone()).await?;
        cap_svc.register(t.clone() as Arc<dyn AnyCapability>)?;
        println!(
            "  stream_echo  →  id={}  timeout={}ms",
            t.id(),
            t.meta().timeout_ms
        );
    }

    // Generator — stream, no deps.
    let gen_token = mint_stream::<GeneratorResource, _>(&factory, &manifests, "generator", |_, _| {
        plugins::generator::handler()
    })?;
    if let Some(t) = &gen_token {
        ctx.provide("cap:generate", (*t.as_ref()).clone()).await?;
        cap_svc.register(t.clone() as Arc<dyn AnyCapability>)?;
        println!("  generate  →  id={}  timeout={}ms", t.id(), t.meta().timeout_ms);
    }

    // Track which manifests have all their capabilities provided.
    let provided_caps: HashSet<String> = manifests
        .iter()
        .filter(|m| !matches!(m.plugin.name.as_str(), "echo-cdylib" | "echo-wasm"))
        .flat_map(|m| m.exposes.iter().map(|c| c.name.clone()))
        .collect();

    // Phase 5: Validate graph.
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

    // Phase 6: Start plugins.
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

    // Phase 7: Demo — typed invocation directly on the mint result.
    println!("\n[demo] capability invocations:");

    if let Some(echo_cap) = &echo_token {
        match echo_cap.invoke(json!({"message": "hello", "n": 42})) {
            Ok(v) => println!("  echo: {v}"),
            Err(e) => println!("  echo error: {e}"),
        }
    }
    if let Some(rev_cap) = &reverse_token {
        match rev_cap.invoke(json!("pipeline")) {
            Ok(v) => println!("  reverse: {v}"),
            Err(e) => println!("  reverse error: {e}"),
        }
    }
    if let Some(stream_cap) = &stream_echo_token {
        match stream_cap.open(json!("hello world from stream_echo")) {
            Ok(rx) => {
                println!("  stream_echo:");
                print!("   ");
                print_stream(rx).await;
            }
            Err(e) => println!("  stream_echo error: {e}"),
        }
    }
    if let Some(slow_cap) = &slow_token {
        println!(
            "\n  slow token (timeout={}ms, will timeout):",
            slow_cap.meta().timeout_ms
        );
        match slow_cap.invoke(json!({"hi": "limiter"})) {
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

    // Pipeline via erased capability view.
    println!("\n[pipeline] token-based composition:");
    if let (Some(rev_cap), Some(echo_cap)) = (&reverse_token, &echo_token) {
        let stages = vec![
            pipeline::SyncStage::new(rev_cap.clone() as Arc<dyn AnyCapability>)?,
            pipeline::SyncStage::new(echo_cap.clone() as Arc<dyn AnyCapability>)?,
        ];
        let p = pipeline::Pipeline::new(stages);
        match p.run(json!("hello")) {
            Ok(v) => println!("  reverse(echo(\"hello\")) = {v}"),
            Err(e) => println!("  [pipeline error] {e}"),
        }
    }

    // Generator streaming.
    if let Some(gen_cap) = &gen_token {
        match gen_cap.open(json!("hello there")) {
            Ok(rx) => {
                println!("  generate streaming:");
                print!("   ");
                print_stream(rx).await;
            }
            Err(e) => println!("  generate error: {e}"),
        }
    }

    // Sandbox via typed token.
    println!("\n[sandbox] exec via token (fuel=1_000_000):");
    if let Some(sandbox_cap) = &sandbox_token {
        match sandbox_cap.invoke(json!({
            "path": "plugins/sandbox_programs/hello.wat",
            "fuel": 1_000_000
        })) {
            Ok(v) => println!("  {}", serde_json::to_string_pretty(&v).unwrap()),
            Err(e) => println!("  error: {e}"),
        }
    }

    // Phase 8: HTTP bridge.
    let ctx_clone = ctx.clone();
    let cap_svc_clone = cap_svc.clone();
    let server_handle = tokio::spawn(async move {
        serve(
            "127.0.0.1:3030".parse().unwrap(),
            ctx_clone,
            cap_svc_clone,
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

    // Touch chain_token to keep lint happy when echo-chain wasn't built.
    let _ = chain_token;

    Ok(())
}

fn load_manifests(dir: &str) -> Result<Vec<PluginManifest>, Box<dyn std::error::Error>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let m = PluginManifest::from_path(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        out.push(m);
    }
    out.sort_by(|a, b| a.plugin.name.cmp(&b.plugin.name));
    Ok(out)
}