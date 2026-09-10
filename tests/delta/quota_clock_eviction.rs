//! δ.3 — MockClock-driven quota eviction.
//!
//! P1-D wire-up: the `MockClock` defined in `kernel::clock.rs`
//! was unused before Phase 5 P1-C threaded `Arc<dyn Clock>`
//! through `Capability<R>`. This test verifies that the quota
//! sliding-window eviction logic (which calls `clock.now()`)
//! observes `MockClock::advance`. With a real clock the test
//! would need to actually sleep 60 seconds, which makes it
//! unsuitable for the unit-test layer.
//!
//! Test plan:
//!
//!   1. Mint a counter cap with `calls_per_minute = 2` and a
//!      `MockClock` (not `SystemClock`). Use
//!      `CapabilityFactory::with_clock` so every cap minted
//!      from this factory sees the same mock.
//!   2. Consume both calls (counter == 2 / limit == 2). Third
//!      call denied (`QuotaExceeded`).
//!   3. Advance the mock clock by 61 seconds.
//!   4. The quota window slides: calls in the previous minute
//!      fall outside the cutoff (`now - 60s`) and are evicted.
//!      Calls are again allowed.
//!   5. A single call is enough to demonstrate the slide;
//!      we then consume the budget again to confirm the
//!      counter reset.
use std::sync::Arc;
use std::time::Duration;

use odyssey::host::factory::CapabilityFactory;
use odyssey::kernel::space::CapabilitySpace;
use odyssey::kernel::{MockClock, OperationRights, QuotaSpec};
use odyssey::plugins::test_only::counter::CounterResource;
use serde_json::json;

#[test]
fn mock_clock_advance_slides_quota_window() {
    let space = CapabilitySpace::new();
    let clock = Arc::new(MockClock::new());
    let factory = CapabilityFactory::with_clock(space.clone(), Arc::clone(&clock) as _);

    // Mint with a 2-call/minute quota. Both calls below will
    // be recorded against the mock clock; advancing the mock
    // by 61s simulates a minute elapsing in real time.
    let slot = crate::common::mint_counter_with_quota_clocked(
        &factory,
        "counter_clock",
        QuotaSpec::unlimited().with_calls_per_minute(2),
    );
    let cap_root = odyssey::kernel::Slot::<CounterResource>::new(space.clone(), slot);

    // First two calls succeed.
    assert!(
        cap_root
            .invoke_op(OperationRights::READ, json!({"op": "read"}))
            .is_ok(),
        "call 1 should succeed"
    );
    assert!(
        cap_root
            .invoke_op(OperationRights::READ, json!({"op": "read"}))
            .is_ok(),
        "call 2 should succeed"
    );

    // Third call: quota exhausted.
    let third = cap_root.invoke_op(OperationRights::READ, json!({"op": "read"}));
    assert!(third.is_err(), "call 3 should be quota-denied");
    assert!(
        matches!(
            third.as_ref().unwrap_err(),
            odyssey::kernel::CapabilityError::QuotaExceeded { kind: odyssey::kernel::QuotaKind::Calls, .. }
        ),
        "expected typed QuotaExceeded::Calls variant; got {third:?}"
    );

    // Slide the window forward by 61 seconds — the previous
    // two call stamps are now older than the cutoff and
    // should be evicted on the next try_call call.
    clock.advance(Duration::from_secs(61));

    // The next call must succeed; the budget is replenished.
    let fourth = cap_root.invoke_op(OperationRights::READ, json!({"op": "read"}));
    assert!(
        fourth.is_ok(),
        "call 4 (post-advance) should succeed; got {fourth:?}"
    );

    // One more should also succeed (limit is 2; we've used 1
    // since the slide). The next one is denied again.
    assert!(
        cap_root
            .invoke_op(OperationRights::READ, json!({"op": "read"}))
            .is_ok(),
        "call 5 should succeed"
    );
    let sixth = cap_root.invoke_op(OperationRights::READ, json!({"op": "read"}));
    assert!(sixth.is_err(), "call 6 should be quota-denied; got {sixth:?}");
}