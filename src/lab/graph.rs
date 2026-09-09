//! Lab — Capability Graph (Phase 2 P6).
//!
//! Demonstrates that the runtime can observe the entire capability
//! layout: nodes, namespaces, and the parent → child relationships
//! that `restrict` / `grant` / `transfer` build.
//!
//! ```text
//!   root (counter)
//!     └── counter_a        (READ | WRITE | ADMIN)
//!           └── counter_b  (READ | WRITE)
//!                 └── counter_c  (READ)
//! ```

use crate::capability::{
    graph::CapabilityGraph, CapabilityRights, OperationRights,
};
use crate::lab::boot_counter;
use crate::plugins::counter::CounterResource;
use serde_json::Value;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab :: Capability Graph ==\n");
    let h = boot_counter();

    // Build a small attenuation tree under the boot counter.
    let child_a = h.cspace.restrict::<CounterResource>(
        h.counter_slot,
        CapabilityRights {
            operations: OperationRights::READ
                | OperationRights::WRITE
                | OperationRights::ADMIN,
            timeout_ms: 5000,
        },
        "counter_a".into(),
    )?;
    let child_b = h.cspace.restrict::<CounterResource>(
        child_a,
        CapabilityRights {
            operations: OperationRights::READ | OperationRights::WRITE,
            timeout_ms: 5000,
        },
        "counter_b".into(),
    )?;
    let child_c = h.cspace.restrict::<CounterResource>(
        child_b,
        CapabilityRights {
            operations: OperationRights::READ,
            timeout_ms: 5000,
        },
        "counter_c".into(),
    )?;

    println!("  attenuation tree:");
    println!("    root (counter, all ops) slot={}", h.counter_slot);
    println!("    └── counter_a    slot={child_a}");
    println!("          └── counter_b  slot={child_b}");
    println!("                └── counter_c  slot={child_c}");

    println!("\n  children_of(root): {:?}", h.cspace.children_of(h.counter_slot));
    println!("  children_of(a):    {:?}", h.cspace.children_of(child_a));
    println!("  children_of(b):    {:?}", h.cspace.children_of(child_b));
    println!("  children_of(c):    {:?}", h.cspace.children_of(child_c));

    let graph = CapabilityGraph::from(&h.cspace);
    println!("\n{}", graph.render());

    let _ = Value::Null; // silence unused import noise
    println!("  done.");
    Ok(())
}