//! α.2 — Delegation.
//!
//! A plugin (`BrokerResource`) holding a capability can mint a derived
//! slot via `cspace.restrict`.

use odyssey::capability::{OperationRights, Slot, SlotId};
use odyssey::plugins::broker::BrokerResource;
use odyssey::plugins::counter::CounterResource;
use serde_json::json;

#[test]
fn broker_mints_child_via_restrict() {
    let (space, factory) = crate::common::boot();
    let counter_slot = crate::common::mint_counter(&factory);
    let counter_cap = Slot::<CounterResource>::new(space.clone(), counter_slot)
        .capability()
        .expect("counter cap");

    let broker_slot = crate::common::mint_broker(&factory, counter_cap.clone(), counter_slot, &space);
    let broker = Slot::<BrokerResource>::new(space.clone(), broker_slot);

    // describe — broker reports what it holds.
    let described = broker
        .invoke(json!({"op": "describe"}))
        .expect("describe works");
    assert_eq!(described["held_name"], "counter");
    assert!(described["held_timeout_ms"].as_u64().unwrap() > 0);

    // delegate — broker mints a child slot in the same CSpace.
    let delegated = broker
        .invoke(json!({
            "op": "delegate",
            "name": "counter_via_broker",
            "ops": ["READ"],
        }))
        .expect("delegate works");
    let new_slot_raw = delegated["slot"]
        .as_str()
        .and_then(|s| s.strip_prefix("slot:"))
        .and_then(|s| s.parse::<u64>().ok())
        .expect("slot id present");
    let new_slot = SlotId::new(new_slot_raw);

    // The child slot lives in the CSpace and works as a Counter slot.
    let child = Slot::<CounterResource>::new(space.clone(), new_slot);
    let child_cap = child.capability().expect("child slot populated");
    assert_eq!(child_cap.operations(), OperationRights::READ);
    assert!(child_cap
        .invoke_op(OperationRights::READ, json!({"op":"read"}))
        .is_ok());
    assert!(child_cap
        .invoke_op(OperationRights::WRITE, json!({"op":"increment"}))
        .is_err());
}