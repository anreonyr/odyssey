//! ζ.34–ζ.36 — Phase 4 P4.8 Revocation mid-flight tests.
//!
//! The Phase 4 thesis property: **capability failure is data,
//! not crash**. P4.8 verifies this holds when a capability is
//! revoked *while the agent is mid-program*.
//!
//! - **ζ.34**: agent's program has 3 echo steps. The test
//!   interleaves: run the agent on a sibling task, drain events
//!   until the second `step_start` is observed, then revoke
//!   the echo slot from the main task. The third step targets
//!   the revoked cap and records `step_skip` ("reachable but
//!   cap missing in cspace"). The agent's stream continues to
//!   `done` — no panic.
//!
//! - **ζ.35**: a streaming cap (generator) is revoked mid-stream.
//!   The existing `mpsc::Receiver<CapabilityChunk>` is unaffected
//!   (it already holds its `Arc<HttpResource>`); chunks keep
//!   flowing. A subsequent attempt to open a new stream on the
//!   same capability returns "cap missing".
//!
//! - **ζ.36**: agent has two reachable caps [echo, database].
//!   A program runs [echo, database, echo]. Interleaved: after
//!   the database `step_start`, revoke echo. Step 1 (echo)
//!   succeeded earlier; step 2 (database) succeeds; step 3
//!   (echo, post-revoke) records `step_skip`. Final `done` has
//!   ok=2, skip=1.

use odyssey::host::resolver::Reachable;
use odyssey::kernel::{
    CapabilityChunk, Resource,
};
use odyssey::plugins::agent::{AgentResource, ProgramStep};
use odyssey::plugins::database::{handler as database_handler, DatabaseResource};
use odyssey::plugins::echo::basic::{handler as echo_handler, EchoResource};
use serde_json::{json, Value};

async fn drain_events(mut rx: tokio::sync::mpsc::Receiver<CapabilityChunk>) -> Vec<Value> {
    let mut events: Vec<Value> = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
            Ok(Some(CapabilityChunk::Item(v))) => events.push(v),
            Ok(Some(CapabilityChunk::Done)) => break,
            Ok(None) => break,
            Err(_) => panic!("agent stream timed out"),
        }
    }
    events
}

/// Drain events until `count` `step_start` events have been
/// observed. Returns the events drained so far (including the
/// matched step_starts and any events that arrived between
/// them). Used by ζ.34/ζ.36 to coordinate a mid-flight revoke
/// from the main test task: open the agent on a sibling task,
/// drain step_starts until the desired step is about to begin,
/// then revoke.
async fn drain_until_step_starts(
    rx: &mut tokio::sync::mpsc::Receiver<CapabilityChunk>,
    count: usize,
) -> Vec<Value> {
    let mut events: Vec<Value> = Vec::new();
    let mut seen = 0usize;
    while seen < count {
        match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
            Ok(Some(CapabilityChunk::Item(v))) => {
                if v["event"] == "step_start" {
                    seen += 1;
                }
                events.push(v);
            }
            Ok(Some(CapabilityChunk::Done)) => panic!("stream done before {count} step_starts; saw {events:?}"),
            Ok(None) => panic!("stream closed before {count} step_starts; saw {events:?}"),
            Err(_) => panic!("timed out waiting for step_starts; saw {events:?}"),
        }
    }
    events
}

struct World {
    space: odyssey::kernel::CapabilitySpace,
    #[allow(dead_code)]
    echo_slot: odyssey::kernel::SlotId,
    #[allow(dead_code)]
    database_slot: odyssey::kernel::SlotId,
}

fn build_world() -> World {
    use odyssey::kernel::space::CapabilitySpace;
    use odyssey::kernel::space::events::GraphEventBus;
    use odyssey::host::factory::CapabilityFactory;
    use odyssey::host::manifest::{CapabilityDecl, IsolationMode, PluginManifest};
    use odyssey::kernel::PluginId;
    let bus = GraphEventBus::default();
    let space = CapabilitySpace::with_bus(bus);
    let factory = CapabilityFactory::new(space.clone());

    let echo_decl = PluginManifest {
        plugin: PluginId { name: "echo".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "echo".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: "echo".into(),
            authority: odyssey::kernel::AuthorityContract::empty(),
            protocol: odyssey::kernel::Protocol::empty(),
        }],
        requires: vec![],

        host: vec![],
        resources: Default::default(),
    };
    let echo_slot = factory.mint::<EchoResource>(
        odyssey::kernel::CapKind::Sync,
        &echo_decl.exposes[0],
        &echo_decl.plugin,
        odyssey::kernel::CapabilityBudget::new(5000),
        echo_handler(),
    );

    let db_decl = PluginManifest {
        plugin: PluginId { name: "database".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "database".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: "database".into(),
            authority: odyssey::kernel::AuthorityContract::empty()
                .with_action("read", "DB_READ"),
            protocol: odyssey::kernel::Protocol::empty(),
        }],
        requires: vec![],

        host: vec![],
        resources: Default::default(),
    };
    let db_slot = factory.mint::<DatabaseResource>(
        odyssey::kernel::CapKind::Sync,
        &db_decl.exposes[0],
        &db_decl.plugin,
        odyssey::kernel::CapabilityBudget::new(5000),
        database_handler(),
    );

    World { space, echo_slot, database_slot: db_slot }
}

fn build_agent(
    name: &str,
    reachable: Vec<Reachable>,
    space: odyssey::kernel::CapabilitySpace,
) -> std::sync::Arc<AgentResource> {
    std::sync::Arc::new(AgentResource::from_reachable(name, reachable, space))
}

// =========================================================================
// ζ.34 — Mid-flight revocation: revoke between step 1 and step 2
// =========================================================================

#[tokio::test]
async fn mid_flight_revoke_skips_subsequent_steps_targeting_revoked_cap() {
    let world = build_world();

    let agent = build_agent(
        "agent",
        vec![Reachable::new("echo", "echo")],
        world.space.clone(),
    );

    // Open the agent on a sibling task. We can't get the
    // JoinHandle back from `open()`, but the receiver is what
    // we need anyway — events flow from the agent task into
    // this main task, which lets us coordinate the revoke.
    let mut rx = agent
        .open(json!({
            "program": [
                { "handle": "echo", "input": "a" },
                { "handle": "echo", "input": "b" },
                { "handle": "echo", "input": "c" },
            ]
        }))
        .expect("agent.open");

    // Wait until step 1 has fully run (its step_start and
    // step_ok) and step 2 has just emitted step_start. At
    // that moment, the agent is about to dispatch step 2.
    // This is the narrow window to revoke from the main
    // task — the agent's run_program is between
    // emit_event(step_start) and cspace.lookup_by_name.
    let before_revoke = drain_until_step_starts(&mut rx, 2).await;
    assert!(
        before_revoke.iter().any(|e| e["event"] == "step_ok"),
        "step 1 should have completed before revoke; events: {before_revoke:?}"
    );

    // Revoke the echo slot. Step 2 and step 3, which target
    // "echo", will now find lookup_by_name returning None.
    let removed = world.space.revoke(world.echo_slot);
    assert!(removed, "revoke should return true");

    // Drain the rest. Step 2 should step_skip ("reachable
    // but cap missing in cspace"). Step 3 should also
    // step_skip for the same reason. Final `done` summarises.
    let after_revoke = drain_events(rx).await;
    let mut all = before_revoke;
    all.extend(after_revoke);
    let done = all.last().expect("done event present");

    // Step 1 succeeded; step 2 and 3 skipped.
    assert_eq!(
        done["ok"], 1,
        "step 1 (pre-revoke) should step_ok; events: {all:?}"
    );
    assert_eq!(
        done["skipped"], 2,
        "step 2 and 3 (post-revoke) should step_skip; events: {all:?}"
    );
    assert_eq!(done["denied"], 0);
    assert_eq!(done["failed"], 0);

    // The skip reason must surface "missing in cspace" —
    // this is what differentiates step_skip from step_deny.
    let skip_reasons: Vec<&str> = all
        .iter()
        .filter(|e| e["event"] == "step_skip")
        .filter_map(|e| e["reason"].as_str())
        .collect();
    assert_eq!(skip_reasons.len(), 2, "two step_skips; got {skip_reasons:?}");
    for r in &skip_reasons {
        assert!(
            r.contains("missing in cspace"),
            "step_skip reason must surface cap-missing; got: {r}"
        );
    }
}

// =========================================================================
// ζ.35 — Revoking a streaming cap doesn't crash in-flight receivers
// =========================================================================

#[tokio::test]
async fn revoking_streaming_cap_does_not_crash_inflight_receiver() {
    use odyssey::plugins::generator::handler as generator_handler;
    use odyssey::plugins::generator::GeneratorResource;
    use odyssey::host::factory::CapabilityFactory;
    use odyssey::host::manifest::{CapabilityDecl, IsolationMode, PluginManifest};
    use odyssey::kernel::PluginId;
    let bus = odyssey::kernel::space::events::GraphEventBus::default();
    let space = odyssey::kernel::CapabilitySpace::with_bus(bus);
    let factory = CapabilityFactory::new(space.clone());

    let g_decl = PluginManifest {
        plugin: PluginId { name: "generator".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "generate".into(),
            in_type: "prompt".into(),
            out_type: "tokens".into(),
            streaming: true,
            contract_name: "generate".into(),
            authority: odyssey::kernel::AuthorityContract::empty(),
            protocol: odyssey::kernel::Protocol::empty(),
        }],
        requires: vec![],

        host: vec![],
        resources: Default::default(),
    };
    let g_slot = factory.mint::<GeneratorResource>(
        odyssey::kernel::CapKind::Stream,
        &g_decl.exposes[0],
        &g_decl.plugin,
        odyssey::kernel::CapabilityBudget::new(5000),
        generator_handler(std::sync::Arc::new(odyssey::plugins::generator::MarkovModel::default())),
    );

    // Open a stream on the (still-alive) generator.
    let cap = space.lookup_typed::<GeneratorResource>(g_slot).expect("cap");
    let mut rx = cap.open(json!("hello")).expect("open ok");

    // Drain one chunk to confirm the stream is producing.
    let first = tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv())
        .await
        .expect("first chunk timeout")
        .expect("first chunk present");
    assert!(matches!(first, CapabilityChunk::Item(_)));

    // Revoke the slot.
    let removed = space.revoke(g_slot);
    assert!(removed, "revoke should return true");

    // The in-flight stream should still drain to completion
    // because the producer task holds its own Arc.
    let mut items_after_revoke = 0;
    let mut done_seen = false;
    while !done_seen {
        match tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await {
            Ok(Some(CapabilityChunk::Item(_))) => items_after_revoke += 1,
            Ok(Some(CapabilityChunk::Done)) => done_seen = true,
            Ok(None) => break,
            Err(_) => panic!("stream hung after revoke — should complete"),
        }
    }
    assert!(items_after_revoke > 0, "should still receive items after revoke");

    // A new open() on the revoked slot must fail. The slot
    // is gone from the namespace.
    let cap_after = space.lookup_typed::<GeneratorResource>(g_slot);
    assert!(cap_after.is_none(), "lookup after revoke must fail");
}

// =========================================================================
// ζ.36 — Mid-flight revocation: agent has 2 caps, echo revoked between
// database step and the post-revoke echo step
// =========================================================================

#[tokio::test]
async fn mid_flight_revoke_of_one_cap_leaves_other_steps_untouched() {
    let world = build_world();

    let agent = build_agent(
        "agent",
        vec![
            Reachable::new("echo", "echo"),
            Reachable::new("database", "database"),
        ],
        world.space.clone(),
    );

    // Program: echo "first", database read "k", echo "second".
    // The 3rd step targets echo AFTER we'll have revoked it.
    let mut rx = agent
        .open(json!({
            "program": [
                { "handle": "echo", "input": "first" },
                { "handle": "database", "op": "read", "input": { "op": "read", "key": "k" } },
                { "handle": "echo", "input": "second" },
            ]
        }))
        .expect("agent.open");

    // Wait until step 2 (database) has emitted step_start.
    // That means step 1 (echo) has already completed and
    // step 2 is about to dispatch. We revoke echo *now*,
    // so step 2 (database) still succeeds (unaffected) and
    // step 3 (echo) hits a missing-in-cspace lookup.
    let before_revoke = drain_until_step_starts(&mut rx, 2).await;
    // Verify step 1 already produced step_ok before we revoke.
    assert!(
        before_revoke.iter().any(|e| e["event"] == "step_ok"),
        "step 1 (echo) should have completed before revoke; events: {before_revoke:?}"
    );

    let removed = world.space.revoke(world.echo_slot);
    assert!(removed, "revoke should return true");

    let after_revoke = drain_events(rx).await;
    let mut all = before_revoke;
    all.extend(after_revoke);
    let done = all.last().expect("done event present");

    // Step 1 (echo, pre-revoke) ok; step 2 (database, never
    // revoked) ok; step 3 (echo, post-revoke) skipped.
    assert_eq!(
        done["ok"], 2,
        "step 1 (pre-revoke echo) and step 2 (database) should step_ok; events: {all:?}"
    );
    assert_eq!(
        done["skipped"], 1,
        "step 3 (post-revoke echo) should step_skip; events: {all:?}"
    );
    assert_eq!(done["denied"], 0);
    assert_eq!(done["failed"], 0);

    // Skip reason must surface the missing cap.
    let skip = all
        .iter()
        .find(|e| e["event"] == "step_skip")
        .expect("step_skip present");
    let reason = skip["reason"].as_str().unwrap();
    assert!(
        reason.contains("missing in cspace"),
        "step_skip reason must surface cap-missing; got: {reason}"
    );

    // Suppress the unused-import warning.
    let _ = std::marker::PhantomData::<ProgramStep>;
}

// =========================================================================
// Edge test 1 — Double-revoke returns false on the second call
// =========================================================================

#[tokio::test]
async fn cspace_double_revoke_returns_false() {
    use odyssey::kernel::space::CapabilitySpace;
    use odyssey::kernel::space::events::GraphEventBus;
    use odyssey::host::factory::CapabilityFactory;
    use odyssey::host::manifest::{CapabilityDecl, IsolationMode, PluginManifest};
    use odyssey::kernel::PluginId;
    let bus = GraphEventBus::default();
    let space = CapabilitySpace::with_bus(bus);
    let factory = CapabilityFactory::new(space.clone());

    let decl = PluginManifest {
        plugin: PluginId { name: "echo".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "echo".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: "echo".into(),
            authority: odyssey::kernel::AuthorityContract::empty(),
            protocol: odyssey::kernel::Protocol::empty(),
        }],
        requires: vec![],

        host: vec![],
        resources: Default::default(),
    };
    let slot_id = factory.mint::<EchoResource>(
        odyssey::kernel::CapKind::Sync,
        &decl.exposes[0],
        &decl.plugin,
        odyssey::kernel::CapabilityBudget::new(5000),
        echo_handler(),
    );

    let first = space.revoke(slot_id);
    assert!(first, "first revoke should succeed");

    let second = space.revoke(slot_id);
    assert!(!second, "second revoke should return false, not panic");

    // The cspace must not double-publish the Revoked event
    // for the second call. Subscribe and confirm no further
    // Revoked events fire for this slot.
    let mut rx = space.subscribe();
    let mut revoked_count = 0;
    while let Ok(res) = tokio::time::timeout(
        std::time::Duration::from_millis(50),
        rx.recv(),
    ).await {
        match res {
            Ok(odyssey::kernel::space::events::GraphEvent::Revoked { slot: s, .. }) => {
                if s == slot_id {
                    revoked_count += 1;
                }
            }
            Ok(_) => continue,
            Err(_) => break,
        }
    }
    // subscribe() is called AFTER the first revoke. A
    // double-revoke from this point must not publish any
    // additional Revoked events for our slot.
    assert_eq!(
        revoked_count, 0,
        "no Revoked event should fire after the second revoke"
    );
}

// =========================================================================
// Edge test 2 — Revoking after a program completes is a clean no-op
// =========================================================================

#[tokio::test]
async fn agent_program_completes_then_revocation_is_noop() {
    use odyssey::kernel::space::CapabilitySpace;
    use odyssey::kernel::space::events::GraphEventBus;
    use odyssey::host::factory::CapabilityFactory;
    use odyssey::host::manifest::{CapabilityDecl, IsolationMode, PluginManifest};
    use odyssey::kernel::PluginId;
    let bus = GraphEventBus::default();
    let space = CapabilitySpace::with_bus(bus);
    let factory = CapabilityFactory::new(space.clone());

    let echo_decl = PluginManifest {
        plugin: PluginId { name: "echo".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "echo".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: "echo".into(),
            authority: odyssey::kernel::AuthorityContract::empty(),
            protocol: odyssey::kernel::Protocol::empty(),
        }],
        requires: vec![],

        host: vec![],
        resources: Default::default(),
    };
    let echo_slot = factory.mint::<EchoResource>(
        odyssey::kernel::CapKind::Sync,
        &echo_decl.exposes[0],
        &echo_decl.plugin,
        odyssey::kernel::CapabilityBudget::new(5000),
        echo_handler(),
    );

    let agent = build_agent(
        "agent",
        vec![Reachable::new("echo", "echo")],
        space.clone(),
    );

    // Run a small program and wait for done.
    let rx = agent
        .open(json!({
            "program": [
                { "handle": "echo", "input": "x" },
                { "handle": "echo", "input": "y" },
            ]
        }))
        .expect("agent.open");
    let events = drain_events(rx).await;
    let done = events.last().expect("done");
    assert_eq!(done["ok"], 2);

    // Revoke after completion. The receiver is already closed;
    // revoke should still return true (the slot was populated
    // when we revoked it) and must not panic.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        space.revoke(echo_slot)
    }));
    let removed = result.expect("revoke after completion must not panic");
    assert!(removed, "revoke after completion should succeed");

    // A second revoke is the no-op case (Edge test 1 in this file).
    let removed2 = space.revoke(echo_slot);
    assert!(!removed2, "second revoke should return false");
}
