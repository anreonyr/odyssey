//! Lab — Quota (Phase 2: rate-limit authority).
//!
//! Demonstrates that the kernel-level `QuotaState` rejects calls
//! after the per-minute budget is exhausted.
//!
//! ```text
//! Counter (root)
//!   │  restrict with calls_per_minute = 3
//!   ▼
//! Counter (rate-limited)
//! ```
//!
//! Three sync calls succeed; the fourth returns a quota error.

use crate::capability::{CapabilityBudget, CapabilityRights, OperationRights, QuotaSpec};
use crate::host::factory::CapabilityFactory;
use crate::lab::boot_counter;
use crate::plugins::counter::CounterResource;
use serde_json::json;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab :: Quota ==\n");
    let h = boot_counter();

    // Restrict the root with a calls-per-minute budget.
    let spec = QuotaSpec::unlimited().with_calls_per_minute(3);
    let budget = CapabilityBudget::with_spec(5000, spec);
    let factory = CapabilityFactory::new(h.cspace.clone());
    let decl = crate::host::manifest::CapabilityDecl {
        name: "counter_rate_limited".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
    };
    let pid = crate::host::manifest::PluginId {
        name: "counter".into(),
        version: "0.1.0".into(),
    };
    let limited_slot = factory.mint::<CounterResource>(
        crate::capability::CapKind::Sync,
        &decl,
        &pid,
        budget,
        crate::plugins::counter::handler(),
    );
    let limited = crate::capability::Slot::<CounterResource>::new(h.cspace.clone(), limited_slot);
    let limited_cap = limited.capability().expect("limited populated");
    println!(
        "  limited slot={} ops={:?} calls_per_minute={}",
        limited_slot,
        limited_cap.operations(),
        limited_cap.meta().quota.calls_per_minute
    );

    // Three calls succeed.
    println!("\n  --- three calls under the limit ---");
    for i in 1..=3 {
        let r = limited.invoke_op(OperationRights::READ, json!({"op": "read"}));
        match r {
            Ok(v) => println!("  ✓ call {i}: {v}"),
            Err(e) => println!("  ✗ call {i}: {e}"),
        }
    }

    // Fourth call is denied.
    println!("\n  --- fourth call exceeds the limit ---");
    let r = limited.invoke_op(OperationRights::READ, json!({"op": "read"}));
    match r {
        Ok(v) => println!("  ✗ call 4 unexpectedly succeeded: {v}"),
        Err(e) => println!("  ✓ call 4 denied: {e}"),
    }

    println!("\n  done.");
    let _ = CapabilityRights::root(5000); // silence unused
    Ok(())
}