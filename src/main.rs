//! Odyssey — seL4-style capability kernel, manifest-driven boot.
//!
//! ## Boot order
//!
//!   Phase 1  Parse manifests       → collect metadata only
//!   Phase 2  Provide core services  → ctx.provide(factory, cap_svc, registry)
//!   Phase 3  Mint all tokens        → factory.mint_* per capability, in dependency order
//!   Phase 4  Provide tokens         → ctx.provide("cap:{name}", token)
//!   Phase 5  Validate graph         → fail-fast on any missing provider
//!   Phase 6  Start plugins          → ctx.plugin(...); cordis resolves inject
//!   Phase 7  Demo harness           → call capabilities via tokens
//!   Phase 8  HTTP bridge            → serves capability_service enumerate
//!
//! The ordering in Phase 3–4 (mint then provide, before plugin start) is
//! critical: cordis fibers will stay PENDING until every `inject` dep has
//! been provided. Without Phase 4 ahead of Phase 6, plugins never activate.

mod capability;
mod dispatcher;
mod http_bridge;
mod manifest;
mod pipeline;
mod plugins;
mod registry;

use std::collections::{HashMap, HashSet};
use std::io::Write as _;
use std::sync::Arc;

use capability::{CapabilityBudget, CapabilityService, CapabilityToken, StreamInvoke, SyncInvoke};
use dispatcher::CapabilityFactory;
use http_bridge::serve;
use manifest::PluginManifest;
use plugins::{
    echo::echo_plugin, echo_chain::echo_chain_plugin, generator::generator_plugin,
    reverse::reverse_plugin, sandbox::sandbox_plugin, slow::slow_plugin,
    stream_echo::stream_echo_plugin,
};
use registry::Registry;
use serde_json::json;

const MANIFEST_DIR: &str = "plugins";

// ---------------------------------------------------------------------------
// Handler dispatch — main orchestrates, plugins own implementations
// ---------------------------------------------------------------------------

/// Build a sync handler for a plugin by name. Token dependencies are
/// resolved from `tokens_by_plugin` (caller must have minted them already).
fn build_sync_handler(
    plugin_name: &str,
    tokens_by_plugin: &HashMap<String, Vec<Arc<CapabilityToken>>>,
) -> Result<Arc<dyn SyncInvoke>, String> {
    match plugin_name {
        "echo" => Ok(plugins::echo::handler()),
        "reverse" => Ok(plugins::reverse::handler()),
        "slow" => Ok(plugins::slow::handler()),
        "sandbox" => Ok(plugins::sandbox::handler()),
        "echo-chain" => {
            // echo-chain wraps echo: must find the already-minted echo token.
            let echo = tokens_by_plugin
                .get("echo")
                .and_then(|v| v.first())
                .ok_or_else(|| "echo-chain requires echo token to be minted first".to_string())?;
            Ok(plugins::echo_chain::handler(echo.clone()))
        }
        "echo-cdylib" | "echo-wasm" => Err(format!("{plugin_name}: deferred")),
        other => Err(format!("no sync handler for plugin '{other}'")),
    }
}

fn build_stream_handler(plugin_name: &str) -> Result<Arc<dyn StreamInvoke>, String> {
    match plugin_name {
        "stream_echo" => Ok(plugins::stream_echo::handler()),
        "generator" => Ok(plugins::generator::handler()),
        other => Err(format!("no stream handler for plugin '{other}'")),
    }
}

/// Print a streaming capability's output. Stops at `Done`.
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

    // factory.clone() shares inner Arc state; cap_svc/registry similarly.
    ctx.provide("capability_factory", factory.clone()).await?;
    ctx.provide("capability_service", (*cap_svc).clone()).await?;
    ctx.provide("registry", registry.clone()).await?;
    println!("[main] core services provided");

    // Phase 3: Mint tokens for every declared capability, in manifest order.
    //          Plugin modules own the handler implementations; main only
    //          asks for them by plugin name.
    println!("\n[mint] capability tokens:");
    let mut tokens_by_plugin: HashMap<String, Vec<Arc<CapabilityToken>>> = HashMap::new();

    for m in &manifests {
        let plugin_id = m.plugin.clone();
        let timeout_ms = m.resources.timeout_ms.unwrap_or(5000);

        for cap_decl in &m.exposes {
            let budget = CapabilityBudget::new(timeout_ms);
            let token = if cap_decl.streaming {
                let handler = match build_stream_handler(&m.plugin.name) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("[skip] {e}");
                        continue;
                    }
                };
                factory.mint_stream(cap_decl, &plugin_id, budget, handler)
            } else {
                let handler = match build_sync_handler(&m.plugin.name, &tokens_by_plugin) {
                    Ok(h) => h,
                    Err(e) => {
                        eprintln!("[skip] {e}");
                        continue;
                    }
                };
                factory.mint_sync(cap_decl, &plugin_id, budget, handler)
            };

            println!(
                "  {}  →  id={}  timeout={}ms",
                cap_decl.name,
                token.id(),
                token.meta().timeout_ms
            );

            // Phase 4: Provide token into context so cordis inject can find it.
            // (*token).clone() = inner CapabilityToken clone (the type V
            // cordis requires). Arc<dyn SyncInvoke> inside means the clone
            // shares the actual handler; only meta + budget Arc are copied.
            let cap_key = format!("cap:{}", cap_decl.name);
            ctx.provide(cap_key.as_str(), (*token).clone()).await?;

            tokens_by_plugin
                .entry(m.plugin.name.clone())
                .or_default()
                .push(token);
        }

        registry.register(m.clone())?;
    }

    // Phase 5: Validate that every "consumes" capability has a provider.
    //          Fail-fast: abort boot before starting plugins.
    println!("\n[graph] validating capability dependencies:");
    let provided_caps: HashSet<String> = tokens_by_plugin
        .values()
        .flat_map(|tokens| tokens.iter().map(|t| t.name().to_string()))
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
    let plugin_fibers: Vec<_> = plugin_list
        .iter()
        .filter(|(name, _)| tokens_by_plugin.contains_key(*name))
        .map(|(name, plugin)| (*name, ctx.plugin(plugin.clone(), None)))
        .collect();

    for (name, handle) in &plugin_fibers {
        match handle.join().await {
            Ok(()) => println!("  ✓ {name} activated"),
            Err(e) => eprintln!("  ✗ {name} failed: {e}"),
        }
    }

    // Phase 7: Demo harness — call capabilities via tokens, no string lookup.
    println!("\n[demo] capability invocations:");

    let echo_token: Arc<CapabilityToken> = ctx.require("cap:echo")?;
    let reverse_token: Arc<CapabilityToken> = ctx.require("cap:reverse")?;
    let slow_token: Arc<CapabilityToken> = ctx.require("cap:slow")?;

    match echo_token.invoke(json!({"message": "hello", "n": 42})) {
        Ok(v) => println!("  echo: {v}"),
        Err(e) => println!("  echo error: {e}"),
    }

    match reverse_token.invoke(json!("pipeline")) {
        Ok(v) => println!("  reverse: {v}"),
        Err(e) => println!("  reverse error: {e}"),
    }

    // Streaming via token.
    if let Ok(rx) = ctx
        .require::<CapabilityToken>("cap:stream_echo")?
        .stream(json!("hello world from stream_echo"))
    {
        println!("  stream_echo:");
        print!("   ");
        print_stream(rx).await;
    }

    // Slow token: handler sleeps 200ms with budget 50ms — must reject.
    println!("\n  slow token (timeout={}ms, will timeout):", slow_token.meta().timeout_ms);
    match slow_token.invoke(json!({"hi": "limiter"})) {
        Ok(v) => println!("  [unexpected success] {v}"),
        Err(e) => println!("  [expected timeout] {e}"),
    }

    // Factory mint snapshots.
    println!("\n[budget] minted token snapshots:");
    for snap in factory.snapshots() {
        println!(
            "  {}@{}: id={} timeout={}ms",
            snap.plugin.name, snap.plugin.version, snap.id, snap.timeout_ms
        );
    }

    // Pipeline via tokens.
    println!("\n[pipeline] token-based composition:");
    let reverse_tok: Arc<CapabilityToken> = ctx.require("cap:reverse")?;
    let echo_tok: Arc<CapabilityToken> = ctx.require("cap:echo")?;
    let p = pipeline::Pipeline::new(vec![
        pipeline::SyncStage::invoke(reverse_tok),
        pipeline::SyncStage::invoke(echo_tok),
    ]);
    match p.run(json!("hello")) {
        Ok(v) => println!("  reverse(echo(\"hello\")) = {v}"),
        Err(e) => println!("  [pipeline error] {e}"),
    }

    // Generator streaming.
    if let Ok(rx) = ctx
        .require::<CapabilityToken>("cap:generate")?
        .stream(json!("hello there"))
    {
        println!("  generate streaming:");
        print!("   ");
        print_stream(rx).await;
    }

    // Sandbox via token.
    println!("\n[sandbox] exec via token (fuel=1_000_000):");
    let sandbox_token: Arc<CapabilityToken> = ctx.require("cap:exec")?;
    match sandbox_token.invoke(json!({
        "path": "plugins/sandbox_programs/hello.wat",
        "fuel": 1_000_000
    })) {
        Ok(v) => println!("  {}", serde_json::to_string_pretty(&v).unwrap()),
        Err(e) => println!("  error: {e}"),
    }

    // Phase 8: HTTP bridge. The shutdown future is consumed by axum's
    // graceful-shutdown path; the server task then completes, and we await
    // it to know when shutdown finished. We do NOT register a separate
    // ctrl_c handler in main — the server owns it.
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