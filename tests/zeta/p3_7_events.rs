//! ζ.12–ζ.16 — Phase 3 P3.7 Graph Events tests.
//!
//! Verify that the cspace and boot publish `GraphEvent`s for
//! every graph mutation. Subscribers see the full timeline in
//! publish order; the bus is non-blocking (no subscribers =
//! events are dropped, no panic).
//!
//! - **ζ.12** — Single mint fires one `Minted` event with the
//!   right plugin/slot/capability/contract.
//! - **ζ.13** — `revoke_tree` fires `Revoked` per cleared slot
//!   plus one `RevokeTree` with the total.
//! - **ζ.14** — `grant` / `restrict` / `transfer` fire
//!   `Derived` events with the correct `DeriveKind`.
//! - **ζ.15** — Multi-subscriber semantics: every subscriber
//!   gets every event.
//! - **ζ.16** — Full lifecycle: mint → activate → shutdown in
//!   reverse order; the recorded event sequence matches the
//!   expected boot shape.

use odyssey::capability::{
    events::{DeriveKind, GraphEvent},
    Capability, CapabilityBudget, CapabilityRights, CapabilitySpace, CapKind, OperationRights,
    Resource, SlotId,
};
use odyssey::kernel::factory::CapabilityFactory;
use odyssey::kernel::manifest::{CapabilityDecl, PluginId};
use odyssey::kernel::manifest_builder::ManifestBuilder as MB;
use odyssey::plugins::test_only::counter::{handler as counter_handler, CounterResource};

const TIMEOUT_MS: u32 = 5000;

// =========================================================================
// ζ.12 — single mint fires Minted
// =========================================================================

#[test]
fn mint_fires_minted_event() {
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    let mut rx = space.subscribe();

    let m = MB::new("counter", "counter", "counter")
        .in_type("object")
        .out_type("object")
        .action("read", "READ")
        .build();
    let slot = factory.mint::<CounterResource>(
        CapKind::Sync,
        &m.exposes[0],
        &m.plugin,
        CapabilityBudget::new(TIMEOUT_MS),
        counter_handler(),
    );

    let ev = rx
        .try_recv()
        .expect("event delivered to subscribed receiver");
    match ev {
        GraphEvent::Minted { plugin, slot: s, capability, contract } => {
            assert_eq!(plugin, m.plugin);
            assert_eq!(s, slot);
            assert_eq!(capability, "counter");
            assert_eq!(contract, "counter");
        }
        other => panic!("expected Minted, got {other:?}"),
    }

    // After the one event, the channel should be empty.
    assert!(rx.try_recv().is_err(), "no further events");
}

// =========================================================================
// ζ.13 — revoke_tree fires Revoked per slot + one RevokeTree
// =========================================================================

#[test]
fn revoke_tree_fires_revoked_per_slot_plus_one_revoke_tree() {
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    let mut rx = space.subscribe();

    let (root, _) = mint_counter(&factory, "counter");
    let read = space
        .restrict::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_read".into(),
        )
        .unwrap();
    let write = space
        .restrict::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ | OperationRights::WRITE,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_write".into(),
        )
        .unwrap();
    // Drain Derived events so the assertions below look at
    // only the revoke sequence.
    while rx.try_recv().is_ok() {}

    let n = space.revoke_tree(root);
    assert_eq!(n, 3);

    // Expect three Revoked (read, write, root — order not
    // guaranteed by the tree walk) plus one RevokeTree.
    let mut revoked_slots: Vec<SlotId> = Vec::new();
    let mut revoke_tree_count = 0usize;
    for _ in 0..4 {
        match rx.try_recv().expect("event queued") {
            GraphEvent::Revoked { slot, .. } => revoked_slots.push(slot),
            GraphEvent::RevokeTree { root: r, total } => {
                assert_eq!(r, root);
                assert_eq!(total, 3);
                revoke_tree_count += 1;
            }
            other => panic!("expected Revoked/RevokeTree, got {other:?}"),
        }
    }
    assert_eq!(revoke_tree_count, 1, "exactly one RevokeTree event");
    assert_eq!(revoked_slots.len(), 3, "three Revoked events");
    for s in [root, read, write] {
        assert!(revoked_slots.contains(&s), "missing Revoked for slot {s}");
    }
}

// =========================================================================
// ζ.14 — derive paths fire Derived with the right kind
// =========================================================================

#[test]
fn derive_paths_fire_distinct_kinds() {
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    let mut rx = space.subscribe();
    let (root, _) = mint_counter(&factory, "counter");

    let _g = space
        .grant::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_g".into(),
        )
        .unwrap();
    let _r = space
        .restrict::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: TIMEOUT_MS,
            },
            "counter_r".into(),
        )
        .unwrap();
    let _t = space
        .transfer::<CounterResource>(
            root,
            CapabilityRights {
                operations: OperationRights::READ,
                timeout_ms: TIMEOUT_MS,
            },
        )
        .unwrap();

    // Drain and collect the Derived events.
    let mut kinds: Vec<DeriveKind> = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let GraphEvent::Derived { kind, .. } = ev {
            kinds.push(kind);
        }
    }
    assert_eq!(
        kinds,
        vec![
            DeriveKind::Grant,
            DeriveKind::Restrict,
            DeriveKind::Transfer,
        ],
        "Grant/Restrict/Transfer produce distinct Derived events in publish order"
    );
}

// =========================================================================
// ζ.15 — multi-subscriber semantics
// =========================================================================

#[test]
fn every_subscriber_gets_every_event() {
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    let mut rx1 = space.subscribe();
    let mut rx2 = space.subscribe();
    assert_eq!(space.events().receiver_count(), 2);

    let (root, _) = mint_counter(&factory, "counter");

    // Both subscribers see the same Minted event.
    let e1 = rx1.try_recv().expect("rx1");
    let e2 = rx2.try_recv().expect("rx2");
    assert!(matches!(e1, GraphEvent::Minted { slot, .. } if slot == root));
    assert!(matches!(e2, GraphEvent::Minted { slot, .. } if slot == root));

    // After revoke, both subscribers see the Revoked.
    space.revoke_tree(root);
    // rx1: drain Revoked + RevokeTree
    let mut rx1_events = vec![rx1.try_recv().unwrap()];
    if let GraphEvent::Revoked { .. } = rx1_events[0] {
        rx1_events.push(rx1.try_recv().unwrap()); // RevokeTree
    }
    // rx2: drain Revoked + RevokeTree
    let mut rx2_events = vec![rx2.try_recv().unwrap()];
    if let GraphEvent::Revoked { .. } = rx2_events[0] {
        rx2_events.push(rx2.try_recv().unwrap()); // RevokeTree
    }
    // Same set on both subscribers.
    assert_eq!(rx1_events.len(), 2);
    assert_eq!(rx2_events.len(), 2);
}

// =========================================================================
// ζ.16 — full boot lifecycle (mint → activate → shutdown) event sequence
// =========================================================================

#[test]
fn full_lifecycle_event_sequence() {
    // This test simulates the boot pipeline (without the
    // cordis/HTTP machinery) and asserts the event sequence
    // matches the expected shape: mint per plugin (Minted
    // x N), activate per plugin (PluginActivated x N),
    // shutdown markers, then per-plugin teardown in reverse
    // mint order.
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    let mut rx = space.subscribe();

    // Three "plugins" in a topological order:
    //   counter (no deps)
    //   echo (no deps)
    //   echo-chain (requires counter, echo)
    // For the test we don't need real deps; we just mint three
    // caps and arrange them in the same mint order boot would.
    let m_counter = MB::new("counter", "counter", "counter")
        .action("read", "READ")
        .build();
    let m_echo = MB::new("echo", "echo", "echo")
        .action("echo", "EXECUTE")
        .build();
    let m_chain = MB::new("echo_chain", "echo_chain", "echo_chain")
        .action("chain", "EXECUTE")
        .build();
    let plugins = vec![m_counter, m_echo, m_chain];
    let slot_ids: Vec<SlotId> = plugins
        .iter()
        .map(|m| {
            factory.mint::<CounterResource>(
                CapKind::Sync,
                &m.exposes[0],
                &m.plugin,
                CapabilityBudget::new(TIMEOUT_MS),
                counter_handler(),
            )
        })
        .collect();

    // Activate phase — publish PluginActivated per plugin.
    for m in &plugins {
        let _ = space
            .events()
            .publish(GraphEvent::PluginActivated { plugin: m.plugin.clone() });
    }

    // Shutdown phase.
    let _ = space.events().publish(GraphEvent::ShutdownStarted);
    // Reverse mint order teardown (P3.6).
    for (i, m) in plugins.iter().enumerate().rev() {
        let _ = space
            .events()
            .publish(GraphEvent::PluginDeactivated { plugin: m.plugin.clone() });
        space.revoke_tree(slot_ids[i]);
    }
    let _ = space.events().publish(GraphEvent::ShutdownCompleted {
        remaining_slots: space.len(),
    });

    // Drain and verify the event timeline.
    let mut events: Vec<GraphEvent> = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    // Expected shape:
    //   Minted (counter)
    //   Minted (echo)
    //   Minted (echo-chain)
    //   PluginActivated (counter)
    //   PluginActivated (echo)
    //   PluginActivated (echo-chain)
    //   ShutdownStarted
    //   PluginDeactivated (echo-chain)   <- last to mint, first to die
    //   Revoked (echo-chain)
    //   RevokeTree (echo-chain, 1)
    //   PluginDeactivated (echo)
    //   Revoked (echo)
    //   RevokeTree (echo, 1)
    //   PluginDeactivated (counter)
    //   Revoked (counter)
    //   RevokeTree (counter, 1)
    //   ShutdownCompleted { remaining_slots: 0 }
    let names_in_mint_order: Vec<&str> = plugins.iter().map(|m| m.plugin.name.as_str()).collect();
    let expected_count = 3 * 4 /* mint+activate+deactivate+revoke_tree */
        + 3 /* per-plugin revoke */
        + 2; /* shutdown markers */
    assert_eq!(events.len(), expected_count, "event count");

    // Minted events in mint order.
    for (i, name) in names_in_mint_order.iter().enumerate() {
        match &events[i] {
            GraphEvent::Minted { plugin, capability, .. } => {
                assert_eq!(&plugin.name, name);
                assert_eq!(capability, name);
            }
            other => panic!("event[{i}] expected Minted({name}), got {other:?}"),
        }
    }

    // PluginActivated in mint order.
    for (i, name) in names_in_mint_order.iter().enumerate() {
        match &events[3 + i] {
            GraphEvent::PluginActivated { plugin } => {
                assert_eq!(&plugin.name, name);
            }
            other => panic!(
                "event[{}] expected PluginActivated({}), got {:?}",
                3 + i,
                name,
                other
            ),
        }
    }

    // ShutdownStarted at position 6.
    assert!(matches!(events[6], GraphEvent::ShutdownStarted));

    // Per-plugin shutdown: deactivated, revoked, revoke_tree.
    // Reverse mint order: chain, echo, counter.
    let mut idx = 7;
    for name in names_in_mint_order.iter().rev() {
        match &events[idx] {
            GraphEvent::PluginDeactivated { plugin } => {
                assert_eq!(&plugin.name, name);
            }
            other => panic!("event[{idx}] expected PluginDeactivated({name}), got {other:?}"),
        }
        idx += 1;
        match &events[idx] {
            GraphEvent::Revoked { capability: Some(c), .. } => assert_eq!(c, name),
            other => panic!("event[{idx}] expected Revoked({name}), got {other:?}"),
        }
        idx += 1;
        match &events[idx] {
            GraphEvent::RevokeTree { total, .. } => assert_eq!(*total, 1),
            other => panic!("event[{idx}] expected RevokeTree, got {other:?}"),
        }
        idx += 1;
    }

    // ShutdownCompleted at the end.
    match &events[idx] {
        GraphEvent::ShutdownCompleted { remaining_slots } => {
            assert_eq!(*remaining_slots, 0, "all slots revoked");
        }
        other => panic!("event[{idx}] expected ShutdownCompleted, got {other:?}"),
    }
}

// =========================================================================
// Helpers
// =========================================================================

fn mint_counter(factory: &CapabilityFactory, cap_name: &str) -> (SlotId, std::sync::Arc<Capability<CounterResource>>) {
    let decl = CapabilityDecl {
        name: cap_name.into(),
        in_type: "object".into(),
        out_type: "object".into(),
        streaming: false,
        ..Default::default()
    };
    let pid = PluginId { name: "counter".into(), version: "0.1.0".into() };
    let slot = factory.mint::<CounterResource>(
        CapKind::Sync,
        &decl,
        &pid,
        CapabilityBudget::new(TIMEOUT_MS),
        counter_handler(),
    );
    let typed = factory.space().lookup_typed::<CounterResource>(slot).unwrap();
    (slot, typed)
}