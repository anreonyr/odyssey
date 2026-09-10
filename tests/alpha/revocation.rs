//! α.4 — Revocation.
//!
//! `cspace.revoke(slot_id)` makes subsequent `Slot::invoke` calls
//! fail; the slot reference itself stays valid (no panic).

use odyssey::capability::Slot;
use odyssey::plugins::test_only::counter::CounterResource;
use serde_json::json;

#[test]
fn lookup_fails_after_revoke() {
    let (space, factory) = crate::common::boot();
    let slot_id = crate::common::mint_counter(&factory);
    let slot = Slot::<CounterResource>::new(space.clone(), slot_id);

    // Sanity: pre-revoke, slot works.
    assert!(slot.invoke(json!({"op":"read"})).is_ok());

    // Revoke.
    assert!(space.revoke(slot_id));

    // The slot reference stays valid (no panic); lookup fails closed.
    let after = slot.capability();
    assert!(after.is_none());
    let err = slot.invoke(json!({"op":"read"})).unwrap_err();
    assert!(err.contains("slot") && err.contains("empty"), "got: {err}");

    // Revocation is idempotent.
    assert!(!space.revoke(slot_id));
}