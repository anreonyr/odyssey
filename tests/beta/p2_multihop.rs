//! β.1 — Multi-hop delegation (extends P2).
//!
//! Three brokers chained through `cspace.restrict` preserve the
//! `rights(c) ⊆ rights(b) ⊆ rights(a) ⊆ rights(root)` invariant.

use std::sync::Arc;

use odyssey::capability::{Capability, CapabilityRights, OperationRights, Slot, SlotId};
use odyssey::plugins::test_only::broker::BrokerResource;
use odyssey::plugins::test_only::counter::CounterResource;
use serde_json::json;

#[test]
fn subset_invariant_holds_three_hops() {
    let (space, factory) = crate::common::boot();
    let root_slot = crate::common::mint_counter(&factory);
    let root = Slot::<CounterResource>::new(space.clone(), root_slot)
        .capability()
        .expect("root");

    // root → a (READ | WRITE | ADMIN) → b (READ | WRITE) → c (READ)
    let a_id = space
        .restrict::<CounterResource>(
            root_slot,
            CapabilityRights {
                operations: OperationRights::READ
                    | OperationRights::WRITE
                    | OperationRights::ADMIN,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "cap_a".into(),
        )
        .unwrap();
    let a: Arc<Capability<CounterResource>> =
        Slot::new(space.clone(), a_id).capability().unwrap();

    let broker_a = crate::common::mint_broker(&factory, a.clone(), a_id, &space);
    let delegated = Slot::<BrokerResource>::new(space.clone(), broker_a)
        .invoke(json!({"op": "delegate", "name": "cap_b", "ops": ["READ", "WRITE"]}))
        .unwrap();
    let b_id = SlotId::new(
        delegated["slot"]
            .as_str()
            .and_then(|s| s.strip_prefix("slot:"))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap(),
    );
    let b: Arc<Capability<CounterResource>> =
        Slot::new(space.clone(), b_id).capability().unwrap();

    let broker_b = crate::common::mint_broker(&factory, b.clone(), b_id, &space);
    let delegated = Slot::<BrokerResource>::new(space.clone(), broker_b)
        .invoke(json!({"op": "delegate", "name": "cap_c", "ops": ["READ"]}))
        .unwrap();
    let c_id = SlotId::new(
        delegated["slot"]
            .as_str()
            .and_then(|s| s.strip_prefix("slot:"))
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap(),
    );
    let c: Arc<Capability<CounterResource>> =
        Slot::new(space.clone(), c_id).capability().unwrap();

    let root_ops = root.operations();
    let a_ops = a.operations();
    let b_ops = b.operations();
    let c_ops = c.operations();

    assert!(root_ops.contains(a_ops), "root ⊇ a");
    assert!(a_ops.contains(b_ops), "a ⊇ b");
    assert!(b_ops.contains(c_ops), "b ⊇ c");

    // c holds only READ; increment (WRITE) is denied.
    assert!(c.invoke_op(OperationRights::READ, json!({"op": "read"})).is_ok());
    assert!(c.invoke_op(OperationRights::WRITE, json!({"op": "increment"})).is_err());
    assert!(b.invoke_op(OperationRights::WRITE, json!({"op": "increment"})).is_ok());
    assert!(a.invoke_op(OperationRights::ADMIN, json!({"op": "reset"})).is_ok());

    // Attempting to amplify c with a bit it doesn't hold is rejected.
    let amp = space.restrict::<CounterResource>(
        c_id,
        CapabilityRights {
            operations: OperationRights::READ | OperationRights::WRITE,
            timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
        },
        "cap_amp".into(),
    );
    assert!(amp.is_err());
}