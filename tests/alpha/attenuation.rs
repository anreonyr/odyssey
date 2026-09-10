//! α.3 — Restrict / attenuation.
//!
//! `cspace.restrict(child ⊇ parent)` returns `AttenuationViolation`.
//! Same invariant applies to `grant` and `transfer`.

use odyssey::kernel::{CapabilityError, CapabilityRights, OperationRights};
use odyssey::plugins::test_only::counter::CounterResource;

#[test]
fn amplification_rejected_across_all_ops() {
    let (space, factory) = crate::common::boot();
    let root_slot = crate::common::mint_counter(&factory);

    // Restrict to READ | WRITE.
    let child_id = space
        .restrict::<CounterResource>(
            root_slot,
            CapabilityRights {
                operations: OperationRights::READ | OperationRights::WRITE,
                timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
            },
            "counter_partial".into(),
        )
        .expect("restrict down ok");

    // Attempt to *amplify* back to ALL — must fail.
    let amplify = space.restrict::<CounterResource>(
        child_id,
        CapabilityRights {
            operations: OperationRights::ALL,
            timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
        },
        "counter_amplified".into(),
    );
    assert!(matches!(
        amplify,
        Err(CapabilityError::AttenuationViolation { .. })
    ));

    // Same with `grant`.
    let grant = space.grant::<CounterResource>(
        child_id,
        CapabilityRights {
            operations: OperationRights::EXECUTE | OperationRights::ADMIN,
            timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
        },
        "counter_grant_attempt".into(),
    );
    assert!(matches!(grant, Err(CapabilityError::AttenuationViolation { .. })));

    // Same with `transfer`.
    let transfer = space.transfer::<CounterResource>(
        child_id,
        CapabilityRights {
            operations: OperationRights::ADMIN,
            timeout_ms: crate::common::DEFAULT_TIMEOUT_MS,
        },
    );
    assert!(matches!(
        transfer,
        Err(CapabilityError::AttenuationViolation { .. })
    ));
}