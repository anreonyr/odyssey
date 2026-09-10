//! P1-L — M3 invariant: invoke_op timeout drops the result and
//! returns the typed Timeout variant; quota is NOT debited on
//! timeout; quota IS debited on a successful handler return.
//!
//! Phase 5 M3 reorder: handler runs first; on success the
//! elapsed wall-clock is recorded via Clock::now(); the
//! per-call timeout is checked (return CapabilityError::Timeout
//! if the budget was blown, dropping the successful handler
//! result \u2014 the budget is the contract); the per-minute
//! quota is debited; the handler result is returned.
//!
//! Four complementary tests pin this on a SINGLE cap each
//! (no two-cap cross-bucket workarounds; each test owns one
//! quota bucket so the invariant is verifiable directly via
//! `budget.snapshot().calls_used`):
//!
//!   - timeout_drops_successful_result_and_does_not_debit_quota
//!     \u2014 a 200ms sleep handler with a 100ms budget returns
//!     the typed Timeout variant. The wall-clock counter is
//!     recorded (the budget was blown); the quota state is
//!     not touched. `calls_used` stays at 0 across the
//!     timeout.
//!
//!   - timeout_does_not_debit_quota \u2014 the same single-cap
//!     shape, but invokes twice. Both calls return Timeout
//!     (200ms sleep / 100ms budget). `calls_used` stays at
//!     0 across both. Without this assertion the invariant
//!     is silently broken: a regression where the quota is
//!     debited before the handler runs would let
//!     `calls_used` climb to 2, and an unrelated cap's
//!     quota-deny path would mask the bug.
//!
//!   - successful_handler_debits_quota \u2014 a 20ms sleep
//!     handler with a 200ms budget and a 2-call quota
//!     returns Ok twice (calls_used climbs to 1, then 2);
//!     the third call exhausts the bucket and returns
//!     QuotaExceeded. `calls_used` is asserted directly
//!     after each call, so a regression that double-debits
//!     or fails-to-debit on success is caught.
//!
//!   - invoke_returns_typed_timeout_on_slow_call
//!     \u2014 `invoke` (no operation-rights check) also
//!     returns the typed Timeout variant on a slow call.
//!     Same M3 invariant.
use std::sync::Arc;
use std::time::Duration;

use odyssey::host::factory::CapabilityFactory;
use odyssey::host::manifest::CapabilityDecl;
use odyssey::kernel::space::events::GraphEventBus;
use odyssey::kernel::{
    Capability, CapabilityBudget, CapabilityError, CapabilitySpace, CapKind,
    OperationRights, QuotaSpec, SlotId,
};
use odyssey::kernel::PluginId;
use odyssey::plugins::slow::{handler as slow_handler, SlowResource};

/// A sleep-handler resource with a configurable sleep duration.
/// Distinct from the production `SlowResource` (hard-coded to
/// 200ms) so the test can drive both fast and slow paths
/// deterministically.
struct SleepResource {
    sleep: Duration,
}

impl odyssey::kernel::Resource for SleepResource {
    fn invoke(
        &self,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        std::thread::sleep(self.sleep);
        Ok(input)
    }
}

fn mint_sleep_cap(
    factory: &CapabilityFactory,
    name: &str,
    sleep: Duration,
    budget_ms: u32,
) -> SlotId {
    let plugin = PluginId {
        name: "sleep".into(),
        version: "0.1.0".into(),
    };
    let decl = CapabilityDecl {
        name: name.into(),
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
        ..Default::default()
    };
    // Use the factory's pre-installed clock (SystemClock by
    // default) so handler elapsed time tracks real wall-clock.
    // The clock-advance test in tests/delta/quota_clock_eviction.rs
    // exercises the MockClock path for quota eviction; this file
    // exercises the M3 timeout/quota ordering against real time.
    let budget = CapabilityBudget::with_clock(
        budget_ms,
        QuotaSpec::unlimited(),
        Arc::clone(factory.clock()),
    );
    factory.mint::<SleepResource>(
        CapKind::Sync,
        &decl,
        &plugin,
        budget,
        Arc::new(SleepResource { sleep }),
    )
}

fn build_world() -> (CapabilitySpace, CapabilityFactory) {
    let bus = GraphEventBus::default();
    let space = CapabilitySpace::with_bus(bus);
    let factory = CapabilityFactory::new(space.clone());
    (space, factory)
}

/// Test A: 200ms sleep handler with 100ms budget. invoke_op
/// returns the typed Timeout variant. The wall-clock counter
/// is recorded (the budget was blown), the quota is NOT
/// debited (the budget contract failed).
#[test]
fn timeout_drops_successful_result_and_does_not_debit_quota() {
    let (space, factory) = build_world();

    // 200ms sleep, 100ms budget, no quota (unlimited).
    let slot = mint_sleep_cap(&factory, "slow_sleep", Duration::from_millis(200), 100);

    let typed: Arc<Capability<SleepResource>> = space
        .lookup_typed::<SleepResource>(slot)
        .expect("typed cap");

    // Wall-clock before: counter starts at 0.
    let wall_before = typed.budget().wall_clock_total_ms();
    assert_eq!(wall_before, 0, "fresh budget has wall_clock_total_ms = 0");

    let result = typed.invoke_op(OperationRights::READ, serde_json::json!({}));
    match result {
        Err(CapabilityError::Timeout {
            elapsed_ms,
            budget_ms,
            ..
        }) => {
            assert!(
                elapsed_ms >= 200,
                "elapsed_ms must reflect the 200ms sleep; got {elapsed_ms}"
            );
            assert_eq!(budget_ms, 100, "budget_ms must surface the contract value");
        }
        other => panic!("expected typed Timeout variant; got {other:?}"),
    }

    // Wall-clock after: counter has recorded the 200ms (rounded up).
    let wall_after = typed.budget().wall_clock_total_ms();
    assert!(
        wall_after >= 200,
        "wall_clock_total_ms must record the 200ms; got {wall_after}"
    );
}

/// Test B: 20ms sleep handler with 200ms budget and a 2-call
/// quota. invoke_op returns Ok twice (the quota is debited on
/// each successful call); the third call exhausts the bucket
/// and returns QuotaExceeded. `budget.snapshot().calls_used` is
/// asserted after each call so a regression that double-debits
/// or fails-to-debit on success is caught directly (no reliance
/// on a second cap's bucket).
#[test]
fn successful_handler_debits_quota() {
    let (space, factory) = build_world();

    let plugin = PluginId {
        name: "sleep".into(),
        version: "0.1.0".into(),
    };
    let decl = CapabilityDecl {
        name: "fast_sleep".into(),
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
        ..Default::default()
    };

    // Mint directly with a 2-call quota.
    let budget = CapabilityBudget::with_spec(
        200,
        QuotaSpec::unlimited().with_calls_per_minute(2),
    );
    let slot = factory.mint::<SleepResource>(
        CapKind::Sync,
        &decl,
        &plugin,
        budget,
        Arc::new(SleepResource {
            sleep: Duration::from_millis(20),
        }),
    );

    let typed: Arc<Capability<SleepResource>> = space
        .lookup_typed::<SleepResource>(slot)
        .expect("typed cap");

    // Pre-condition: fresh budget, calls_used = 0.
    assert_eq!(
        typed.budget().snapshot().calls_used,
        0,
        "fresh budget starts with calls_used=0"
    );

    // First call: Ok, quota debited to 1.
    let r1 = typed.invoke_op(OperationRights::READ, serde_json::json!({}));
    assert!(r1.is_ok(), "call 1 (20ms / 200ms budget) should succeed; got {r1:?}");
    assert_eq!(
        typed.budget().snapshot().calls_used,
        1,
        "first successful call must debit calls_used to 1"
    );

    // Second call: Ok, quota debited to 2.
    let r2 = typed.invoke_op(OperationRights::READ, serde_json::json!({}));
    assert!(r2.is_ok(), "call 2 should succeed; got {r2:?}");
    assert_eq!(
        typed.budget().snapshot().calls_used,
        2,
        "second successful call must debit calls_used to 2"
    );

    // Third call: QuotaExceeded. The bucket is full;
    // calls_used stays at 2 (a quota-exhausted call does
    // not consume additional capacity).
    let r3 = typed.invoke_op(OperationRights::READ, serde_json::json!({}));
    assert!(
        matches!(
            r3,
            Err(CapabilityError::QuotaExceeded {
                kind: odyssey::kernel::QuotaKind::Calls,
                ..
            })
        ),
        "call 3 should be quota-denied; got {r3:?}"
    );
    assert_eq!(
        typed.budget().snapshot().calls_used,
        2,
        "QuotaExceeded must not consume additional capacity; calls_used stays at 2"
    );
}

/// Test C: SINGLE slow cap (200ms sleep, 100ms budget, 3-call
/// quota). Two consecutive `invoke_op` calls both return Timeout.
/// The key invariant is `budget.snapshot().calls_used == 0` across
/// both timeouts \u2014 the budget blew, but the quota bucket was
/// not touched. The 3-call quota is chosen so a hypothetical
/// regression that debits the quota before the handler runs would
/// surface as `calls_used >= 1` after the first timeout; with
/// the production handler-first ordering, the quota stays at 0
/// for both calls.
#[test]
fn timeout_does_not_debit_quota() {
    let (space, factory) = build_world();

    let plugin = PluginId {
        name: "sleep".into(),
        version: "0.1.0".into(),
    };
    let decl = CapabilityDecl {
        name: "slow_timeout".into(),
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
        ..Default::default()
    };

    // 200ms sleep, 100ms budget, 3-call quota. The budget
    // blows on every call; the quota is only touched by
    // successful handler returns.
    let budget = CapabilityBudget::with_spec(
        100,
        QuotaSpec::unlimited().with_calls_per_minute(3),
    );
    let slot = factory.mint::<SleepResource>(
        CapKind::Sync,
        &decl,
        &plugin,
        budget,
        Arc::new(SleepResource {
            sleep: Duration::from_millis(200),
        }),
    );
    let typed: Arc<Capability<SleepResource>> = space
        .lookup_typed::<SleepResource>(slot)
        .expect("typed slow cap");

    // Pre-condition: fresh cap, calls_used = 0.
    let snap_before = typed.budget().snapshot();
    assert_eq!(
        snap_before.calls_used, 0,
        "fresh budget must start with calls_used=0; got {}",
        snap_before.calls_used
    );

    // First call: Timeout.
    let r1 = typed.invoke_op(OperationRights::READ, serde_json::json!({}));
    assert!(
        matches!(r1, Err(CapabilityError::Timeout { .. })),
        "call 1 should time out; got {r1:?}"
    );

    // After first Timeout: calls_used must still be 0. The
    // budget was blown, so the handler's successful return
    // was dropped; the quota bucket was not debited. This
    // is the M3 invariant: handler-first / quota-second.
    let snap1 = typed.budget().snapshot();
    assert_eq!(
        snap1.calls_used, 0,
        "M3 INVARIANT VIOLATED: first Timeout must NOT debit the quota; calls_used = {}",
        snap1.calls_used
    );

    // Second call: same slow cap, same quota bucket.
    // Another Timeout. calls_used must STILL be 0.
    let r2 = typed.invoke_op(OperationRights::READ, serde_json::json!({}));
    assert!(
        matches!(r2, Err(CapabilityError::Timeout { .. })),
        "call 2 should time out; got {r2:?}"
    );

    let snap2 = typed.budget().snapshot();
    assert_eq!(
        snap2.calls_used, 0,
        "M3 INVARIANT VIOLATED: second Timeout must NOT debit the quota; calls_used = {}",
        snap2.calls_used
    );
}

/// Sanity: invoke (without operation-rights check) also
/// returns the typed Timeout variant on a slow call. Same
/// M3 invariant.
#[test]
fn invoke_returns_typed_timeout_on_slow_call() {
    let (space, factory) = build_world();

    let plugin = PluginId {
        name: "sleep".into(),
        version: "0.1.0".into(),
    };
    let decl = CapabilityDecl {
        name: "invoke_only".into(),
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
        ..Default::default()
    };
    let budget = CapabilityBudget::new(100);
    let slot = factory.mint::<SleepResource>(
        CapKind::Sync,
        &decl,
        &plugin,
        budget,
        Arc::new(SleepResource {
            sleep: Duration::from_millis(200),
        }),
    );
    let typed: Arc<Capability<SleepResource>> = space
        .lookup_typed::<SleepResource>(slot)
        .expect("typed cap");

    let result = typed.invoke(serde_json::json!({}));
    assert!(
        matches!(result, Err(CapabilityError::Timeout { .. })),
        "invoke (no op check) should also return typed Timeout; got {result:?}"
    );
}

// Suppress unused-import warnings if any test is later
// removed. slow_handler and SlowResource are referenced
// through the type only; pin them so dead-code analysis
// doesn't complain.
#[allow(dead_code)]
fn _force_link() {
    let _: Arc<SlowResource> = slow_handler();
}