//! Lab 04 — Composition.
//!
//! Question to answer: **capabilities compose into computation, and
//! revocation of an underlying slot fails the composition closed
//! without touching the pipeline itself**.
//!
//! We use two `EchoResource` slots here (a true passthrough) so the
//! pipeline *threads* values across stages. The interesting property
//! is the slot-bound `SyncStage::from_slot` form: every call
//! re-resolves the slot, so `cspace.revoke(...)` propagates without
//! touching the pipeline itself.
//!
//! Steps:
//! 1. Mint two echo slots, A and B.
//! 2. Compose a slot-bound pipeline `A → B`.
//! 3. Run — both stages fire, value flows through unchanged.
//! 4. `cspace.revoke(A)`.
//! 5. Run again — `PipelineError::SlotRevoked`, without touching
//!    the pipeline.
//!
//! Compare: a `SyncStage::Pinned` (built from `Arc<dyn AnyCapability>`)
//! would have kept the cap alive even after revocation; the lab
//! uses the slot-bound form deliberately to make revocation
//! observable end-to-end.

use crate::host::pipeline::{Pipeline, SyncStage};
use crate::lab::boot_counter;
use crate::plugins::echo::EchoResource;
use serde_json::json;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab 04 :: Composition ==\n");
    let h = boot_counter();

    // Mint a second sync resource — Echo (true passthrough) — so the
    // pipeline value actually threads through stages.
    let decl = crate::host::manifest::CapabilityDecl {
        name: "echo_b".into(),
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
        ..Default::default()
    };
    let pid = crate::host::manifest::PluginId {
        name: "echo".into(),
        version: "0.1.0".into(),
    };
    let echo_a_slot = h.factory.mint::<EchoResource>(
        crate::capability::CapKind::Sync,
        &decl,
        &pid,
        crate::capability::CapabilityBudget::new(5000),
        crate::plugins::echo::handler(),
    );
    let echo_b_slot = h.factory.mint::<EchoResource>(
        crate::capability::CapKind::Sync,
        &decl,
        &pid,
        crate::capability::CapabilityBudget::new(5000),
        crate::plugins::echo::handler(),
    );

    println!("  A slot={echo_a_slot}  (echo_a)");
    println!("  B slot={echo_b_slot}  (echo_b)");

    // Build a slot-bound pipeline so revocation propagates.
    let stages = vec![
        SyncStage::from_slot(echo_a_slot, h.cspace.clone())?,
        SyncStage::from_slot(echo_b_slot, h.cspace.clone())?,
    ];
    let pipeline = Pipeline::new(stages);

    println!("\n  --- before revoke ---");
    match pipeline.run(json!({"msg": "hello pipeline"})) {
        Ok(v) => println!("  ✓ pipeline = {v}"),
        Err(e) => println!("  ✗ pipeline failed: {e}"),
    }

    println!("\n  --- revoke A ---");
    let removed = h.cspace.revoke(echo_a_slot);
    println!("  revoke(A) → {removed}");

    println!("\n  --- after revoke ---");
    match pipeline.run(json!({"msg": "should fail"})) {
        Ok(v) => println!("  ✗ pipeline ran: {v}"),
        Err(e) => println!("  ✓ pipeline closed: {e}"),
    }

    println!("\n  done.");
    Ok(())
}