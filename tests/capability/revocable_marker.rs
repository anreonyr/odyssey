//! Round-3 reviewer finding (Phase 4 review loop): the
//! Revocable marker was added in round 2 but unverified by any
//! direct test. This file pins the contract:
//!
//! 1. Cached typed `Arc<Capability<R>>` (the path plugins use via
//!    `Slot::capability()`) must observe `cspace.revoke`.
//! 2. After `revoke`, `is_revoked()` is `true`.
//! 3. After `revoke`, `invoke_op(READ, ...)` returns
//!    `Err(CapabilityError::Revoked(slot))` (Phase 5 M4: typed
//!    match, was: substring match on `"capability revoked"`).
//! 4. A fresh `Arc<Capability<R>>` from `cspace::install` has
//!    `is_revoked() == false` (lifecycle reset).
//! 5. Cloning a typed `Arc<Capability<R>>` shares the marker —
//!    revoking once is observed by every clone.

use std::sync::Arc;

use odyssey::kernel::{Capability, CapabilityError, OperationRights, Slot};
use odyssey::plugins::test_only::counter::CounterResource;

#[test]
fn typed_cap_arc_observes_cspace_revoke() {
    let (space, factory) = crate::common::boot();
    let slot = crate::common::mint_counter(&factory);

    let typed: Arc<Capability<CounterResource>> =
        Slot::new(space.clone(), slot).capability().expect("typed cap");

    assert!(!typed.is_revoked(), "fresh slot must not be revoked");

    // Cache the typed Arc the way a plugin would, then revoke
    // the slot from underneath. Without `Arc<AtomicBool>`, the
    // typed cap would carry a fresh `AtomicBool` and observe
    // nothing — that's the regression this test pins.
    assert!(space.revoke(slot), "revoke must succeed");

    assert!(
        typed.is_revoked(),
        "cached typed Arc must observe cspace revoke"
    );

    let err = typed
        .invoke_op(OperationRights::READ, serde_json::json!({"op": "read"}))
        .expect_err("invoke on revoked cap must error");
    // Phase 5 M4: typed match on CapabilityError::Revoked(slot).
    assert!(
        matches!(err, CapabilityError::Revoked(s) if s == slot),
        "expected typed Revoked(slot) variant; got {err:?}"
    );
}

#[test]
fn typed_cap_clones_share_revocable_marker() {
    let (space, factory) = crate::common::boot();
    let slot = crate::common::mint_counter(&factory);

    let a: Arc<Capability<CounterResource>> =
        Slot::new(space.clone(), slot).capability().expect("typed cap a");
    let b = Arc::clone(&a);

    // Arc::ptr_eq on the marker itself would be too intrusive
    // (the field is private). Instead: a and b must agree on
    // `is_revoked` across a flip — pre-revoke both false,
    // post-revoke both true. If `Clone` allocated a fresh
    // AtomicBool, b would stay false after the revoke.
    assert_eq!(a.is_revoked(), b.is_revoked());

    assert!(space.revoke(slot));

    assert_eq!(a.is_revoked(), b.is_revoked());
    assert!(a.is_revoked());
    assert!(b.is_revoked());
}

#[test]
fn freshly_installed_slot_resets_marker() {
    // After a revoke, a new slot installed over the same handler
    // must start with `revoked == false` (lifecycle reset is the
    // kernel's responsibility, not the holder's).
    let (space, factory) = crate::common::boot();
    let slot = crate::common::mint_counter(&factory);

    let first: Arc<Capability<CounterResource>> =
        Slot::new(space.clone(), slot).capability().expect("first cap");

    assert!(space.revoke(slot));
    assert!(first.is_revoked());

    // cspace::install on the same slot id (or a freshly minted
    // one over the same handler) re-issues a cap. The marker
    // must reset so the new cap is usable.
    let slot2 = crate::common::mint_counter(&factory);
    let second: Arc<Capability<CounterResource>> =
        Slot::new(space.clone(), slot2).capability().expect("second cap");
    assert!(!second.is_revoked());
}