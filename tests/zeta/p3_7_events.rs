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

use odyssey::kernel::{
    space::events::{DeriveKind, GraphEvent},
    Capability, CapabilityBudget, CapabilityRights, CapabilitySpace, CapKind, OperationRights, SlotId,
};
use odyssey::host::factory::CapabilityFactory;
use odyssey::host::manifest::{CapabilityDecl};
use odyssey::kernel::PluginId;
use odyssey::host::manifest::ManifestBuilder as MB;
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

    // Expected shape (in causal order):
    //   Phase A — mint:    1 × Minted per plugin (in mint order)
    //   Phase B — boot:    1 × PluginActivated per plugin (mint order)
    //   Phase C — shutdown markers:
    //                     1 × ShutdownStarted
    //                     per plugin (reverse mint order):
    //                        PluginDeactivated
    //                        Revoked
    //                        RevokeTree
    //                     1 × ShutdownCompleted
    //
    // Asserts check the *category* of each event at each
    // position, not the hardcoded total — a future event added
    // to the bus would extend the timeline but shouldn't break
    // the existing shape contract.
    let names_in_mint_order: Vec<&str> =
        plugins.iter().map(|m| m.plugin.name.as_str()).collect();
    let n = names_in_mint_order.len();

    // Sanity check: the category count is correct, regardless
    // of any new event variants added to `GraphEvent`. If this
    // is the only assertion that breaks when we add e.g.
    // `MigrateStarted`, the fix is to extend the timeline
    // categories below — not to bump a magic number.
    assert_eq!(
        events.len(),
        n + n + 1 + n * 3 + 1,
        "event timeline length: {n} mint + {n} activated + \
         1 ShutdownStarted + {n}*3 teardown + 1 ShutdownCompleted"
    );

    // --- Phase A: minted events in mint order ---
    let mint_range = 0..n;
    for (i, name) in names_in_mint_order.iter().enumerate() {
        match &events[mint_range.start + i] {
            GraphEvent::Minted { plugin, capability, .. } => {
                assert_eq!(&plugin.name, name, "Minted plugin order");
                assert_eq!(capability, name, "Minted capability name");
            }
            other => panic!(
                "event[{}] expected Minted({name}), got {other:?}",
                mint_range.start + i
            ),
        }
    }

    // --- Phase B: PluginActivated in mint order ---
    let activate_range = n..(2 * n);
    for (i, name) in names_in_mint_order.iter().enumerate() {
        match &events[activate_range.start + i] {
            GraphEvent::PluginActivated { plugin } => {
                assert_eq!(&plugin.name, name, "Activated plugin order");
            }
            other => panic!(
                "event[{}] expected PluginActivated({name}), got {other:?}",
                activate_range.start + i
            ),
        }
    }

    // --- Phase C: shutdown markers + per-plugin teardown ---
    let shutdown_start_idx = 2 * n;
    match &events[shutdown_start_idx] {
        GraphEvent::ShutdownStarted => {}
        other => panic!(
            "event[{shutdown_start_idx}] expected ShutdownStarted, got {other:?}"
        ),
    }

    // Per-plugin teardown in reverse mint order:
    //   PluginDeactivated → Revoked → RevokeTree
    let mut idx = shutdown_start_idx + 1;
    for name in names_in_mint_order.iter().rev() {
        match &events[idx] {
            GraphEvent::PluginDeactivated { plugin } => {
                assert_eq!(&plugin.name, name, "Deactivated plugin order");
            }
            other => panic!(
                "event[{idx}] expected PluginDeactivated({name}), got {other:?}"
            ),
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
    // idx is now exactly events.len() - 1; ensure no stray events.
    assert_eq!(idx + 1, events.len(), "no events after ShutdownCompleted");
}

// =========================================================================
// ζ.21 — Multi-slot plugin shutdown emits per-slot events
// =========================================================================
//
//     Complement to ζ.16 (single-slot plugins). For a plugin
//     with N [[exposes]] blocks, the shutdown loop emits:
//       1  PluginDeactivated
//     + N  (1 Revoked + 1 RevokeTree) per slot
//     = 1 + 2N events for the per-plugin portion of the
//     shutdown sequence (plus the global ShutdownStarted /
//     ShutdownCompleted bracketing). The single-slot ζ.16 case
//     is N=1: 1 + 2*1 = 3, matching its `n*3` formula.
//     Per-slot granularity gives audit logs a record of each
//     individual cap revocation.

#[test]
fn multi_slot_plugin_shutdown_emits_per_slot_events() {
    let space = CapabilitySpace::new();
    let factory = CapabilityFactory::new(space.clone());
    let mut rx = space.subscribe();

    // Mint two caps for the same logical plugin. The
    // shutdown loop treats a multi-slot plugin as one
    // PluginDeactivated followed by per-slot revoke_tree
    // calls.
    let (_slot_a, _) = mint_counter(&factory, "counter_a");
    let (_slot_b, _) = mint_counter(&factory, "counter_b");

    let plugin = PluginId { name: "multi".into(), version: "0.1.0".into() };
    let _ = space.events().publish(GraphEvent::PluginActivated {
        plugin: plugin.clone(),
    });

    // Shutdown sequence (mirrors what `lifecycle::shutdown_runtime_plugins`
    // does for a plugin with 2 [[exposes]]).
    let _ = space.events().publish(GraphEvent::ShutdownStarted);
    let _ = space.events().publish(GraphEvent::PluginDeactivated {
        plugin: plugin.clone(),
    });
    let n_a = space.revoke_tree(_slot_a);
    let n_b = space.revoke_tree(_slot_b);
    assert_eq!(n_a, 1, "single-slot revoke_tree returns 1");
    assert_eq!(n_b, 1, "single-slot revoke_tree returns 1");
    let _ = space.events().publish(GraphEvent::ShutdownCompleted {
        remaining_slots: space.len(),
    });

    // Drain and assert the shape.
    let mut events: Vec<GraphEvent> = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }

    // Expected timeline (10 events for 2 slots):
    //   0..2    : 2 × Minted (one per slot)
    //   2       : PluginActivated
    //   3       : ShutdownStarted
    //   4       : PluginDeactivated
    //   5..6    : slot_a → Revoked + RevokeTree
    //   7..8    : slot_b → Revoked + RevokeTree
    //   9       : ShutdownCompleted
    assert_eq!(events.len(), 10, "2× Minted + Activated + Started + Deactivated + 2×(Revoked + RevokeTree) + Completed");

    match &events[0] {
        GraphEvent::Minted { capability, .. } => assert_eq!(capability, "counter_a"),
        other => panic!("events[0] expected Minted(counter_a), got {other:?}"),
    }
    match &events[1] {
        GraphEvent::Minted { capability, .. } => assert_eq!(capability, "counter_b"),
        other => panic!("events[1] expected Minted(counter_b), got {other:?}"),
    }
    assert!(matches!(&events[2], GraphEvent::PluginActivated { .. }));
    assert!(matches!(&events[3], GraphEvent::ShutdownStarted));
    assert!(matches!(&events[4], GraphEvent::PluginDeactivated { .. }));

    // Per-slot Revoked + RevokeTree — order within a slot is
    // Revoked first (the inner revoke emits it), then
    // RevokeTree (emitted by revoke_tree at the end).
    match (&events[5], &events[6]) {
        (GraphEvent::Revoked { .. }, GraphEvent::RevokeTree { total, .. }) => {
            assert_eq!(*total, 1, "single-slot subtree has total=1");
        }
        other => panic!("events[5..7] expected (Revoked, RevokeTree), got {other:?}"),
    }
    match (&events[7], &events[8]) {
        (GraphEvent::Revoked { .. }, GraphEvent::RevokeTree { total, .. }) => {
            assert_eq!(*total, 1, "single-slot subtree has total=1");
        }
        other => panic!("events[7..9] expected (Revoked, RevokeTree), got {other:?}"),
    }
    assert!(matches!(&events[9], GraphEvent::ShutdownCompleted { .. }));
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