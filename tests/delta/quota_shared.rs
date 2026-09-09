//! δ.2 — Quota shared across `restrict`.
//!
//! Derived caps draw from the parent's rate-limit bucket, so the
//! parent's calls debit the same counter the child observes.

use odyssey::capability::{CapabilityRights, OperationRights, QuotaSpec, Slot};
use odyssey::plugins::counter::CounterResource;
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
    assert!(
        third.as_ref().unwrap_err().contains("quota"),
        "expected 'quota' in error, got {third:?}"
    );
}