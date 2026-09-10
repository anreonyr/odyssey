//! α.1 — Authority.
//!
//! A `Capability<R>` consulted with the right bit runs the handler;
//! consulted without it is denied.

use odyssey::kernel::{CapabilityRights, OperationRights, Slot};
use odyssey::plugins::test_only::counter::CounterResource;
use serde_json::json;

#[test]
fn correct_bit_invokes_handler() {
    let (space, factory) = crate::common::boot();
    let root_slot = crate::common::mint_counter(&factory);
    let cap = Slot::<CounterResource>::new(space.clone(), root_slot)
        .capability()
        .expect("root populated");

    // All four operations work on a root cap.
    assert!(cap.invoke_op(OperationRights::READ, json!({"op":"read"})).is_ok());
    assert!(cap.invoke_op(OperationRights::WRITE, json!({"op":"increment"})).is_ok());
    assert!(cap.invoke_op(OperationRights::ADMIN, json!({"op":"reset"})).is_ok());

    // Restrict the cap to READ only.
    let child_id = space
        .restrict::<CounterResource>(
            root_slot,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "counter_readonly".into(),
        )
        .expect("restrict ok");
    let child_cap = Slot::<CounterResource>::new(space.clone(), child_id)
        .capability()
        .expect("child populated");

    assert!(child_cap.invoke_op(OperationRights::READ, json!({"op":"read"})).is_ok());
    assert!(child_cap
        .invoke_op(OperationRights::WRITE, json!({"op":"increment"}))
        .is_err());
    assert!(child_cap
        .invoke_op(OperationRights::ADMIN, json!({"op":"reset"}))
        .is_err());
}