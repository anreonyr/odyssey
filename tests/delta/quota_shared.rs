//! δ.2 — Quota shared across `restrict`.
//!
//! Derived caps draw from the parent's rate-limit bucket, so the
//! parent's calls debit the same counter the child observes.
//! Phase 5 M4: the quota-denial assertion pattern-matches on
//! the typed `CapabilityError::QuotaExceeded` variant instead of
//! substring matching on the rendered error message.

use odyssey::kernel::{CapabilityError, CapabilityRights, OperationRights, QuotaKind, QuotaSpec, Slot};
use odyssey::plugins::test_only::counter::CounterResource;
use serde_json::json;

#[test]
fn restrict_inherits_parent_quota() {
    let (space, factory) = crate::common::boot();
    let root = crate::common::mint_counter_with_quota(
        &factory,
        "counter_q_root",
        QuotaSpec::unlimited().with_calls_per_minute(2),
    );
    let cap_root = Slot::<CounterResource>::new(space.clone(), root);

    // Derive a child via restrict — must NOT mint a fresh quota bucket.
    let child = space
        .restrict::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "counter_q_child".into(),
        )
        .unwrap();
    let cap_child = Slot::<CounterResource>::new(space.clone(), child);

    // Root consumes its 2 calls.
    assert!(cap_root
        .invoke_op(OperationRights::READ, json!({"op": "read"}))
        .is_ok());
    assert!(cap_root
        .invoke_op(OperationRights::READ, json!({"op": "read"}))
        .is_ok());

    // Child has nothing left in the shared bucket.
    let third = cap_child.invoke_op(OperationRights::READ, json!({"op": "read"}));
    assert!(
        third.is_err(),
        "child must hit shared quota denial, got {third:?}"
    );
    // Phase 5 M4: typed match on CapabilityError::QuotaExceeded.
    assert!(
        matches!(third, Err(CapabilityError::QuotaExceeded { kind: QuotaKind::Calls, .. })),
        "expected typed QuotaExceeded::Calls variant; got {third:?}"
    );
}