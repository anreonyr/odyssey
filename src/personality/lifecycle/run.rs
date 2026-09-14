//! Personality / lifecycle / run — the top-level orchestrator.
//!
//! Phase 8 transitional stub. The full orchestrator wires
//! together `boot::load_manifests`, `mint::mint_all`,
//! `serve::spawn_http_bridge`, and `ruin::ruin_runtime_plugins`.
//!
//! In the new architecture the typed mint dispatch lives at the
//! `builtins/` workspace member (each builtin exports its own
//! `mint(plugin, decl) -> Arc<dyn Resource>`). When the binary
//! adopts `builtins::echo`, `builtins::reverse`, etc., the
//! orchestrator iterates them and dispatches.
//!
//! For now `run()` boots an empty cspace and serves HTTP. The
//! typed dispatch gets wired in commit "create builtins/ workspace
//! member".

use std::sync::Arc;

use crate::capability::enforce::space::CapabilitySpace;
use crate::capability::enforce::space::GraphEvent;
use crate::capability::init::new_kernel;
use crate::core::clock::clock::SystemClock;
use crate::personality::composition::resolve::resolve;
use crate::personality::lifecycle::boot::{load_manifests, print_manifests};
use crate::personality::lifecycle::mint::CapabilityFactory;
use crate::personality::lifecycle::ruin::ruin_runtime_plugins;
use crate::personality::lifecycle::serve::spawn_http_bridge;

/// Top-level entry point. Phase 8 transitional: loads zero
/// manifests (the `builtins/` workspace member is empty at this
/// point), creates a kernel-backed cspace + factory, brings up
/// the HTTP bridge, waits for Ctrl-C, tears down.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup.
    let manifests = load_manifests()?;
    print_manifests(&manifests);

    let ctx = cordis::Context::new();
    let clock: Arc<dyn crate::core::clock::clock::Clock> = Arc::new(SystemClock);
    let kernel = new_kernel(clock);
    let cspace: CapabilitySpace = kernel.space().clone();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    ctx.provide("capability_space", cspace.clone()).await?;
    ctx.provide("capability_factory", factory.clone()).await?;
    println!("[main] core services provided");

    // Resolve manifests (empty list → empty plan).
    let plan = resolve(&manifests).map_err(|e| format!("resolver: {e}"))?;

    // 2. Plugins — no builtins wired yet, so no mint.
    println!("[mint] no builtins wired; cspace starts empty");

    // 3. Serve.
    let server_handle =
        spawn_http_bridge("127.0.0.1:3030".parse().unwrap(), cspace.clone());
    eprintln!("\n[main] HTTP bridge up — open http://127.0.0.1:3030/");
    eprintln!("[main] press Ctrl-C to stop");
    let _ = server_handle.await;

    // 4. Teardown.
    cspace.publish_event(GraphEvent::ShutdownStarted);
    ruin_runtime_plugins(&cspace, &plan, &Default::default()).await;
    cspace.publish_event(GraphEvent::ShutdownCompleted {
        remaining_slots: cspace.len(),
    });
    ctx.stop().await;
    Ok(())
}
