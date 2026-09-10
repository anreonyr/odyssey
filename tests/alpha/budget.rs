//! α.5 — Budget.
//!
//! A `Slow` resource exceeds its budget returns a typed
//! `CapabilityError::Timeout`; a generous budget succeeds.
//! Phase 5 M4: the assertion pattern-matches on the typed
//! variant instead of substring matching on the rendered error
//! message (was: err.contains("timeout") || err.contains("budget")).

use odyssey::kernel::{CapabilityError, CapabilityRights, Slot};
use odyssey::plugins::slow::SlowResource;
use serde_json::json;

#[test]
fn tight_budget_times_out() {
    let (space, factory) = crate::common::boot();
    let slow_slot = crate::common::mint_slow(&factory);
    let slow = Slot::<SlowResource>::new(space.clone(), slow_slot);

    // Generous budget — SlowResource sleeps 200ms; the default
    // 5s budget is fine. Restrict down to 100ms while the handler
    // still sleeps 200ms → typed Timeout denial.
    let cap = slow.capability().expect("slow cap");
    let rights = cap.rights();
    assert!(rights.timeout_ms >= 200, "default budget must be ≥ handler sleep");

    let tight_id = space
        .restrict::<SlowResource>(
            slow_slot,
            CapabilityRights {
                operations: rights.operations,
                timeout_ms: 100,
            },
            "slow_tight".into(),
        )
        .expect("restrict ok");
    let tight = Slot::<SlowResource>::new(space.clone(), tight_id);
    let err = tight.invoke(json!({})).unwrap_err();
    // Phase 5 M4: typed match on CapabilityError::Timeout.
    assert!(
        matches!(err, CapabilityError::Timeout { elapsed_ms, budget_ms, .. } if elapsed_ms >= 200 && budget_ms == 100),
        "expected typed Timeout variant with elapsed_ms >= 200 and budget_ms == 100; got {err:?}"
    );

    // Generous budget (root) — succeeds.
    let ok = slow.invoke(json!({}));
    assert!(ok.is_ok(), "generous budget should succeed, got: {ok:?}");
}