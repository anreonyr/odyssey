//! Lab 02 — Delegation.
//!
//! Question to answer: **authority can propagate through a plugin, not
//! only from the host**.
//!
//! Experiment:
//! 1. Host mints the root Counter capability.
//! 2. Broker holds the Counter cap, registered in the same CSpace.
//! 3. Broker's `delegate` op mints a child slot via `cspace.restrict`.
//! 4. The child slot lives in the CSpace like any other — query by
//!    name, hold a typed `Slot<R>`, revoke through the slot.
//!
//! Verification:
//!   * broker.describe → reports the held cap id and ops.
//!   * broker.delegate("counter_child", [READ]) → mint child slot.
//!   * child.read succeeds; child.increment denied (no WRITE bit).

use crate::capability::{OperationRights, Slot};
use crate::lab::{assert_authority, boot_counter, counter_cap, rights_summary};
use crate::plugins::broker::{broker_plugin, BrokerResource};
use crate::plugins::counter::CounterResource;
use serde_json::json;

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== Lab 02 :: Delegation ==\n");
    let h = boot_counter();

    // Broker holds the Counter cap.
    let counter_cap_arc = counter_cap(&h);
    println!(
        "  broker holds: cap={} ops={:?}",
        counter_cap_arc.id(),
        counter_cap_arc.operations()
    );

    // Mint the broker capability; the broker resource closes over
    // (counter_cap, counter_slot_id, cspace).
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
    let broker_slot_id = h.factory.mint::<BrokerResource>(
        crate::capability::CapKind::Sync,
        &decl,
        &pid,
        crate::capability::CapabilityBudget::new(5000),
        crate::plugins::broker::handler(counter_cap_arc.clone(), h.counter_slot, h.cspace.clone()),
    );
    let broker_slot = Slot::<BrokerResource>::new(h.cspace.clone(), broker_slot_id);
    let broker_cap = broker_slot.capability().ok_or("broker slot empty")?;
    println!("  broker slot={broker_slot_id} cap={}", broker_cap.id());

    // 1) describe — the broker reports what it holds.
    println!("\n  --- broker.describe ---");
    match broker_slot.invoke(json!({"op": "describe"})) {
        Ok(v) => println!("  ✓ {v}"),
        Err(e) => println!("  ✗ {e}"),
    }

    // 2) delegate — mint a child with only READ.
    println!("\n  --- broker.delegate(\"counter_via_broker\", [READ]) ---");
    let delegate_res = broker_slot.invoke(json!({
        "op": "delegate",
        "name": "counter_via_broker",
        "ops": ["READ"],
    }));
    let new_slot_id: u64 = match delegate_res {
        Ok(v) => {
            let raw = v
                .get("slot")
                .and_then(|s| s.as_str())
                .and_then(|s| s.strip_prefix("slot:"))
                .and_then(|n| n.parse::<u64>().ok())
                .ok_or("missing slot id")?;
            println!("  ✓ broker minted slot {v}");
            raw
        }
        Err(e) => return Err(format!("broker.delegate failed: {e}").into()),
    };

    // The new slot is addressable by name in the CSpace.
    let child_slot = Slot::<CounterResource>::new(
        h.cspace.clone(),
        crate::capability::SlotId::new(new_slot_id),
    );
    let child_cap = child_slot.capability().ok_or("child slot empty")?;
    println!("  child slot={new_slot_id} rights={}", rights_summary(&child_cap));

    assert_authority(
        "child read",
        &child_cap,
        OperationRights::READ,
        json!({"op": "read"}),
        true,
    );
    assert_authority(
        "child increment",
        &child_cap,
        OperationRights::WRITE,
        json!({"op": "increment"}),
        false,
    );

    // 3) Kernel refuses amplification through the broker.
    //    We restrict the broker slot to READ-only first. Then we
    //    ask the stripped broker to delegate WRITE; the kernel's
    //    `cspace.restrict` rejects because READ ⊄ READ|WRITE.
    println!("\n  --- broker authority is bounded by its own slot ---");
    let broker_strip = h.cspace.restrict::<BrokerResource>(
        broker_slot_id,
        crate::capability::CapabilityRights {
            operations: OperationRights::READ,
            timeout_ms: 5000,
        },
        "broker_strip".into(),
    )?;
    let broker_strip_slot = Slot::<BrokerResource>::new(h.cspace.clone(), broker_strip);
    let broker_strip_cap = broker_strip_slot.capability().expect("strip slot");
    println!(
        "  stripped broker slot={broker_strip} rights={}",
        rights_summary_broker(&broker_strip_cap)
    );
    match broker_strip_slot.invoke(json!({
        "op": "delegate",
        "name": "counter_amplified",
        "ops": ["READ", "WRITE"],
    })) {
        Ok(_) => println!("  ✗ broker amplified authority (BUG)"),
        Err(e) => println!("  ✓ broker denied: {e}"),
    }

    println!("\n  done.");
    // broker_plugin is exported but not started — the lab exercises
    // the resource directly. Touch it here so the warning stays off
    // the bin while we keep the export for boot later.
    let _ = broker_plugin();
    Ok(())
}

fn rights_summary_broker(cap: &crate::capability::Capability<BrokerResource>) -> String {
    format!(
        "{:?} timeout={}ms",
        cap.operations(),
        cap.rights().timeout_ms
    )
}