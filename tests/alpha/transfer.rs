//! Bonus (α.2 follow-on) — `transfer` moves and clears the source.
//!
//! Unlike `grant`, `transfer` clears the source slot. The destination
//! slot receives the capability; the source slot becomes empty (the
//! `Slot<R>` reference itself stays valid — no panic).

use odyssey::capability::{CapabilityRights, OperationRights, Slot};
use odyssey::plugins::counter::CounterResource;
use serde_json::json;

#[test]
fn source_cleared_after_transfer() {
    let (space, factory) = crate::common::boot();
    let root_slot = crate::common::mint_counter(&factory);
    let root = Slot::<CounterResource>::new(space.clone(), root_slot);

    let moved_id = space
        .transfer::<CounterResource>(
            root_slot,
            CapabilityRights {
                operations: OperationRights::ALL,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
        )
        .expect("transfer ok");

    // Source is empty.
    assert!(root.capability().is_none());

    // Destination has the cap.
    let moved = Slot::<CounterResource>::new(space.clone(), moved_id);
    assert!(moved.invoke(json!({"op":"read"})).is_ok());
}