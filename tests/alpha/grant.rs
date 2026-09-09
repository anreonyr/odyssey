//! Bonus (α.2 follow-on) — `grant` preserves the source capability.
//!
//! Unlike `transfer`, `grant` mints a child slot without clearing the
//! source. The source cap must still work after the grant; the child
//! cap holds the (subset) rights the parent granted.

use odyssey::capability::{CapabilityRights, OperationRights, Slot};
use odyssey::plugins::counter::CounterResource;
use serde_json::json;

#[test]
fn source_survives_grant() {
    let (space, factory) = crate::common::boot();
    let root_slot = crate::common::mint_counter(&factory);
    let root = Slot::<CounterResource>::new(space.clone(), root_slot);

    // Source works before grant.
    assert!(root.invoke(json!({"op":"read"})).is_ok());

    // Grant a child.
    let child_id = space
        .grant::<CounterResource>(
            root_slot,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "counter_grant".into(),
        )
        .expect("grant ok");

    // Source still works (grant ≠ transfer).
    assert!(root.invoke(json!({"op":"read"})).is_ok());

    // Child works with restricted rights.
    let child = Slot::<CounterResource>::new(space.clone(), child_id);
    let child_cap = child.capability().expect("child populated");
    assert!(child_cap
        .invoke_op(OperationRights::READ, json!({"op": "read"}))
        .is_ok());
    assert!(child_cap
        .invoke_op(OperationRights::WRITE, json!({"op": "increment"}))
        .is_err());
}