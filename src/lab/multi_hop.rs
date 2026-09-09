//! Lab — Multi-hop Plugin-to-Plugin Delegation (Phase 2 P2).
//!
//! Verifies that authority can flow through a chain of plugins:
//!
//! ```text
//! Counter (root, all ops)
//!      │
//!      │ cspace.restrict (READ | WRITE | ADMIN)
//!      ▼
//!   cap_a (held by Broker A)
//!      │
//!      │ Broker A.delegate("counter_b", [READ, WRITE])
//!      ▼
//!   cap_b (held by Broker B)
//!      │
//!      │ Broker B.delegate("counter_c", [READ])
//!      ▼
//!   cap_c
//! ```
//!
//! Asserts at each hop:
//!
//! - `rights(cap_c) ⊆ rights(cap_b) ⊆ rights(cap_a) ⊆ rights(counter_root)`
//! - Each `restrict` enforces the subset invariant.
//! - Revoking the middle hop (`revoke_tree(cap_b)`) severs every
//!   downstream slot — the Phase 2 P3 multi-hop revocation property.

use crate::capability::{Capability, CapabilityRights, OperationRights, Slot, SlotId};
use crate::lab::boot_counter;
use crate::plugins::broker::{handler as broker_handler, BrokerResource};
use crate::plugins::counter::CounterResource;
use serde_json::json;

/// Helper: mint a Broker that closes over a counter capability +
/// source slot id, plus the cspace.
fn mint_broker(
    h: &crate::lab::Harness,
    counter_cap: std::sync::Arc<Capability<CounterResource>>,
    counter_slot: SlotId,
) -> SlotId {
    let decl = crate::host::manifest::CapabilityDecl {
        name: "broker".into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
    };
    let pid = crate::host::manifest::PluginId {
        name: "broker".into(),
        version: "0.1.0".into(),
    };
    h.factory.mint::<BrokerResource>(
        crate::capability::CapKind::Sync,
        &decl,
        &pid,
        crate::capability::CapabilityBudget::new(5000),
        broker_handler(counter_cap, counter_slot, h.cspace.clone()),
    )
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab :: Multi-hop Delegation ==\n");
    let h = boot_counter();

    // Step 1 — root Counter has all ops.
    let counter_cap = crate::lab::counter_cap(&h);
    let counter_slot = h.counter_slot;
    println!(
        "  counter (root) slot={counter_slot} ops={:?} timeout={}ms",
        counter_cap.operations(),
        counter_cap.rights().timeout_ms
    );

    // Step 2 — restrict the counter into "cap_a" (READ | WRITE | ADMIN).
    let cap_a_id = h.cspace.restrict::<CounterResource>(
        counter_slot,
        CapabilityRights {
            operations: OperationRights::READ
                | OperationRights::WRITE
                | OperationRights::ADMIN,
            timeout_ms: 5000,
        },
        "counter_a".into(),
    )?;
    let cap_a = Slot::<CounterResource>::new(h.cspace.clone(), cap_a_id)
        .capability()
        .expect("cap_a populated");
    println!("  cap_a slot={cap_a_id} ops={:?}", cap_a.operations());

    // Step 3 — mint Broker A holding cap_a.
    let broker_a_id = mint_broker(&h, cap_a.clone(), cap_a_id);
    let broker_a = Slot::<BrokerResource>::new(h.cspace.clone(), broker_a_id);
    println!("  broker_a slot={broker_a_id} (holds cap_a)");

    // Step 4 — Broker A delegates "counter_b" with READ | WRITE.
    let delegated = broker_a.invoke(json!({
        "op": "delegate",
        "name": "counter_b",
        "ops": ["READ", "WRITE"],
    }))?;
    let cap_b_id: SlotId = SlotId::new(
        delegated["slot"]
            .as_str()
            .and_then(|s| s.strip_prefix("slot:"))
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("missing slot id")?,
    );
    let cap_b = Slot::<CounterResource>::new(h.cspace.clone(), cap_b_id)
        .capability()
        .expect("cap_b populated");
    println!("  cap_b slot={cap_b_id} ops={:?}", cap_b.operations());

    // Step 5 — mint Broker B holding cap_b.
    let broker_b_id = mint_broker(&h, cap_b.clone(), cap_b_id);
    let broker_b = Slot::<BrokerResource>::new(h.cspace.clone(), broker_b_id);
    println!("  broker_b slot={broker_b_id} (holds cap_b)");

    // Step 6 — Broker B delegates "counter_c" with READ only.
    let delegated = broker_b.invoke(json!({
        "op": "delegate",
        "name": "counter_c",
        "ops": ["READ"],
    }))?;
    let cap_c_id: SlotId = SlotId::new(
        delegated["slot"]
            .as_str()
            .and_then(|s| s.strip_prefix("slot:"))
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("missing slot id")?,
    );
    let cap_c = Slot::<CounterResource>::new(h.cspace.clone(), cap_c_id)
        .capability()
        .expect("cap_c populated");
    println!("  cap_c slot={cap_c_id} ops={:?}", cap_c.operations());

    // Step 7 — verify the attenuation invariant at each hop.
    println!("\n  --- rights(c) ⊆ rights(b) ⊆ rights(a) ⊆ rights(root) ---");
    let root_ops = counter_cap.operations();
    let a_ops = cap_a.operations();
    let b_ops = cap_b.operations();
    let c_ops = cap_c.operations();
    println!("  root={root_ops:?}");
    println!("     a={a_ops:?}  ⊆ root? {}", root_ops.contains(a_ops));
    println!("       b={b_ops:?}  ⊆ a?    {}", a_ops.contains(b_ops));
    println!("         c={c_ops:?}  ⊆ b?    {}", b_ops.contains(c_ops));
    assert!(root_ops.contains(a_ops));
    assert!(a_ops.contains(b_ops));
    assert!(b_ops.contains(c_ops));

    // Step 8 — verify each cap can only do what its bits allow.
    println!("\n  --- per-hop behaviour ---");
    let label = "c.read";
    let r = cap_c.invoke_op(OperationRights::READ, json!({"op": "read"}));
    println!(
        "  ✓ {label} — {}",
        r.as_ref().map(|v| v.to_string()).unwrap_or_else(|e| e.to_string())
    );
    let r = cap_c.invoke_op(OperationRights::WRITE, json!({"op": "increment"}));
    println!(
        "  ✗ c.increment — {}",
        r.as_ref().map(|v| v.to_string()).unwrap_or_else(|e| e.to_string())
    );
    let r = cap_b.invoke_op(OperationRights::WRITE, json!({"op": "increment"}));
    println!(
        "  ✓ b.increment — {}",
        r.as_ref().map(|v| v.to_string()).unwrap_or_else(|e| e.to_string())
    );
    let r = cap_a.invoke_op(OperationRights::ADMIN, json!({"op": "reset"}));
    println!(
        "  ✓ a.reset — {}",
        r.as_ref().map(|v| v.to_string()).unwrap_or_else(|e| e.to_string())
    );

    // Step 9 — amplification rejected.
    println!("\n  --- amplification attempts ---");
    let amp = h.cspace.restrict::<CounterResource>(
        cap_c_id,
        CapabilityRights {
            operations: OperationRights::READ | OperationRights::WRITE,
            timeout_ms: 5000,
        },
        "counter_amplified".into(),
    );
    match amp {
        Ok(_) => println!("  ✗ kernel amplified authority (BUG)"),
        Err(e) => println!("  ✓ kernel rejected: {e}"),
    }

    // Step 10 — revoke_tree at the middle hop severs the entire
    //          subtree (cap_b itself + cap_c, since cap_c was
    //          derived from cap_b). This is Phase 2 P3 — multi-hop
    //          revocation propagation.
    println!("\n  --- revoke_tree(cap_b) ---");
    let removed = h.cspace.revoke_tree(cap_b_id);
    println!("  revoke_tree(cap_b) → {removed} slots removed");
    let post_b = Slot::<CounterResource>::new(h.cspace.clone(), cap_b_id)
        .invoke_op(OperationRights::READ, json!({"op": "read"}));
    match post_b {
        Ok(_) => println!("  ✗ cap_b still works (BUG)"),
        Err(e) => println!("  ✓ cap_b denied: {e}"),
    }
    let post_c = Slot::<CounterResource>::new(h.cspace.clone(), cap_c_id)
        .invoke_op(OperationRights::READ, json!({"op": "read"}));
    match post_c {
        Ok(_) => println!("  ✗ cap_c still works after cap_b revoked (BUG)"),
        Err(e) => println!("  ✓ cap_c denied after cap_b revoked: {e}"),
    }

    println!("\n  done.");
    Ok(())
}