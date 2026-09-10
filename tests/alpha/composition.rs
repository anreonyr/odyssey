//! α.6 — Composition.
//!
//! Slot-bound pipeline stages observe revocation mid-run via
//! `PipelineError::SlotRevoked`.

use odyssey::host::pipeline::{Pipeline, SyncStage};
use serde_json::json;

#[test]
fn pipeline_fails_after_revoke() {
    let (space, factory) = crate::common::boot();
    let echo_a = crate::common::mint_echo(&factory, "echo_a");
    let echo_b = crate::common::mint_echo(&factory, "echo_b");

    // Slot-bound pipeline so each stage re-resolves.
    let pipeline = Pipeline::new(vec![
        SyncStage::from_slot(echo_a, space.clone()).expect("stage a"),
        SyncStage::from_slot(echo_b, space.clone()).expect("stage b"),
    ]);

    // Before revoke — value flows through.
    let v = pipeline.run(json!({"k": "v"})).expect("first run ok");
    assert_eq!(v, json!({"k": "v"}));

    // Revoke A.
    assert!(space.revoke(echo_a));

    // After revoke — SlotRevoked.
    let err = pipeline.run(json!({"k": "v2"})).unwrap_err();
    assert!(
        matches!(err, odyssey::host::pipeline::PipelineError::SlotRevoked(_)),
        "expected SlotRevoked, got: {err}"
    );
}