//! 8-phase boot orchestrator.
//!
//! The orchestrator string-together each phase:
//!   Phase 1  Load manifests → manifests::load_manifests
//!   Phase 2  Provide core services
//!   Phase 3  Resolve capability graph → crate::host::resolver
//!   Phase 4  Mint runtime plugins → crate::runtime::mint
//!   Phase 5  (reserved — was legacy consumes log; now no-op)
//!   Phase 6  Activate runtime plugins → crate::runtime::activate
//!   Phase 7  HTTP bridge + wait → shutdown::spawn_http_bridge
//!   Phase 8  Teardown in reverse mint order → crate::runtime::teardown
//!
//! The phase numbering is preserved across releases even
//! though Phase 5 has been a no-op since the `consumes` field
//! was removed in Phase 6, so external docs and dashboards
//! can keep a stable 8-phase vocabulary.

pub mod manifests;
pub mod phases;
pub mod print;
pub mod shutdown;

use crate::host::factory::CapabilityFactory;
use crate::host::resolver::{resolve, ResolvedPlan};
use crate::kernel::space::events::GraphEvent;
use crate::kernel::CapabilitySpace;

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Phase 1: Parse manifests.
    let manifests = manifests::load_manifests()?;
    print::print_manifests(&manifests);

    // Phase 2: Provide core services.
    let ctx = cordis::Context::new();
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace.clone());

    ctx.provide("capability_space", cspace.clone()).await?;
    ctx.provide("capability_factory", factory.clone()).await?;
    println!("[main] core services provided");

    // Phase 3: Resolve the capability dependency graph.
    let plan: ResolvedPlan = resolve(&manifests).map_err(|e| format!("resolver: {e}"))?;
    println!("\n[resolver]\n{}", plan.render());

    // Phase 4: Mint runtime plugins in resolved order.
    println!("[mint] runtime plugins (in resolved order):");
    let minted = crate::runtime::mint::mint_runtime_plugins(
        &ctx,
        &factory,
        &cspace,
        &plan,
        &manifests,
    )
    .await?;

    // Phase 5: reserved — was the legacy `consumes` log; now a
    // no-op marker so the 8-phase numbering stays stable.

    // Phase 6: Activate runtime plugins in resolved order.
    crate::runtime::activate::activate_runtime_plugins(&ctx, &cspace, &plan).await;

    // Phase 7: HTTP bridge + wait.
    let server_handle = shutdown::spawn_http_bridge("127.0.0.1:3030".parse().unwrap(), cspace.clone());
    eprintln!("\n[main] HTTP bridge up — open http://127.0.0.1:3030/");
    eprintln!("[main] press Ctrl-C to stop");
    let _ = server_handle.await;

    // Phase 8: tear down runtime plugins in reverse mint order.
    cspace.publish_event(GraphEvent::ShutdownStarted);
    crate::runtime::teardown::ruin_runtime_plugins(&cspace, &plan, &minted).await;
    cspace.publish_event(GraphEvent::ShutdownCompleted {
        remaining_slots: cspace.len(),
    });

    ctx.stop().await;
    Ok(())
}
