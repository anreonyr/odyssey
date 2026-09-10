//! δ.1 — Per-minute rate limit.
//!
//! A `Capability` minted with `calls_per_minute = N` denies the
//! (N+1)-th call.

use odyssey::capability::{OperationRights, QuotaSpec, Slot};
use odyssey::plugins::test_only::counter::CounterResource;
use serde_json::json;

#[test]
fn quota_denies_overflow_call() {
    let (space, factory) = crate::common::boot();
    let slot = crate::common::mint_counter_with_quota(
        &factory,
        "counter_q",
        QuotaSpec::unlimited().with_calls_per_minute(2),
    );
    let cap = Slot::<CounterResource>::new(space.clone(), slot);

    // First two calls succeed.
    assert!(cap
        .invoke_op(OperationRights::READ, json!({"op": "read"}))
        .is_ok());
    assert!(cap
        .invoke_op(OperationRights::READ, json!({"op": "read"}))
        .is_ok());

    // Third call denied by quota.
    let third = cap.invoke_op(OperationRights::READ, json!({"op": "read"}));
    assert!(third.is_err(), "expected quota denial, got: {third:?}");
    assert!(
        third.as_ref().unwrap_err().contains("quota"),
        "expected 'quota' in error, got: {third:?}"
    );
}