//! Phase 3 P3.7 — Graph event bus.
//!
//! Every mutation in the capability graph (mint, derive, revoke,
//! revoke-tree) plus every plugin-level lifecycle event
//! (activated, deactivated, shutdown started/completed) is
//! published to a [`GraphEventBus`] backed by
//! `tokio::sync::broadcast`. Subscribers see the full timeline
//! in causal order (broadcast is FIFO from a single sender).
//!
//! ## Why broadcast?
//!
//! - **Non-blocking publish.** `Sender::send` returns
//!   immediately, dropping events only if the channel buffer
//!   overflows. The cspace stays sync; mint never waits on
//!   subscribers.
//! - **Multi-subscriber.** Multiple consumers (HTTP bridge
//!   SSE, log subscriber, test recorder, future audit log)
//!   each get their own `Receiver`. Adding a subscriber
//!   never changes the publisher.
//! - **Tokio-native.** The codebase already pulls tokio for
//!   the HTTP bridge and the boot signal wait; reusing the
//!   same runtime's primitives keeps the dependency surface
//!   flat.
//!
//! ## Event ordering
//!
//! `tokio::sync::broadcast` guarantees that a single sender's
//! events arrive at receivers in the same order they were
//! sent. There is **no** cross-sender ordering — we have one
//! sender per `CapabilitySpace`, so this isn't a concern in
//! practice.
//!
//! ## Backpressure
//!
//! Capacity defaults to `DEFAULT_CAPACITY = 256`. If a
//! receiver lags (doesn't keep up), `try_recv()` returns
//! `Err(Lagged(n))` with the count of skipped events. Tests
//! assert with `try_recv()` rather than `.recv().await` so
//! they don't hang if a previous test consumed events.
//!
//! ## Relationship to existing patterns
//!
//! The HTTP bridge has its own `mpsc::channel` for SSE
//! streaming; that's a separate concern (streaming cap
//! output to clients). Graph events are about the kernel's
//! own mutations, not data plane traffic.

use crate::capability::SlotId;
use crate::kernel::manifest::PluginId;

/// Default channel capacity. 256 events covers any plausible
/// boot or teardown sequence (we emit ≤ 4 events per plugin
/// + 2 shutdown markers, so 64 plugins fit comfortably). Tests
/// with synthetic bursts use a smaller capacity to verify
/// overflow behaviour.
pub const DEFAULT_CAPACITY: usize = 256;

/// How a derived cap was produced from its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeriveKind {
    /// `cspace.grant(...)` — derived slot is a peer's view of
    /// the parent. Source slot unchanged.
    Grant,
    /// `cspace.restrict(...)` — derived slot has strictly
    /// fewer rights than the parent. Source slot unchanged.
    Restrict,
    /// `cspace.transfer(...)` — source slot is cleared, the
    /// derived slot takes over its identity.
    Transfer,
}

/// One graph mutation. The boot pipeline and the cspace both
/// publish these; receivers see the full timeline.
///
/// Variant shape:
///
/// - `Minted { plugin, slot, capability, contract }`: a brand
///   new cap was installed via `factory.mint`. Carries the
///   plugin identity (so audit logs know "who minted") plus
///   the slot id (so downstream consumers can dereference)
///   plus the cap name + contract name (so logs are
///   human-readable).
///
/// - `Derived { parent, child, kind }`: a cap was derived from
///   another via `grant`, `restrict`, or `transfer`. Carries
///   both slot ids and the operation kind. The child's
///   `Minted` event is **not** also emitted — `Derived` is
///   the single canonical event for "a cap appeared".
///
/// - `Revoked { slot, capability }`: a single slot was
///   cleared. `capability` is the cap name if it was
///   registered in the names map, else `None` (e.g. a slot
///   that was already revoked). Empty `Option<String>` avoids
///   needing to re-read cspace from the receiver.
///
/// - `RevokeTree { root, total }`: a `revoke_tree` call
///   revoked `total` slots starting at `root`. The total
///   includes the root and every descendant.
///
/// - `PluginActivated { plugin }`: cordis finished activating
///   a plugin (its handler returned `Ok`).
///
/// - `PluginDeactivated { plugin }`: shutdown reached a
///   plugin; its minted slots are about to be revoked.
///
/// - `ShutdownStarted`: the boot pipeline entered the
///   teardown phase.
///
/// - `ShutdownCompleted { remaining_slots }`: teardown
///   finished; `remaining_slots` is `cspace.len()` at the
///   end (typically 0).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphEvent {
    Minted {
        plugin: PluginId,
        slot: SlotId,
        capability: String,
        contract: String,
    },
    Derived {
        parent: SlotId,
        child: SlotId,
        kind: DeriveKind,
    },
    Revoked {
        slot: SlotId,
        capability: Option<String>,
    },
    RevokeTree {
        root: SlotId,
        total: usize,
    },
    PluginActivated {
        plugin: PluginId,
    },
    PluginDeactivated {
        plugin: PluginId,
    },
    ShutdownStarted,
    ShutdownCompleted {
        remaining_slots: usize,
    },
}

/// Subscription handle. `try_recv` is sync and never blocks
/// (it returns `Err(Empty)` if no events are queued);
/// `recv().await` is async and waits for the next event.
///
/// Re-exported from `tokio::sync::broadcast::Receiver` so
/// callers don't need to import tokio directly.
pub type GraphEventReceiver = tokio::sync::broadcast::Receiver<GraphEvent>;

/// Error returned by `GraphEventReceiver::try_recv`. Re-export
/// so callers can pattern-match without a tokio import.
pub type TryRecvError = tokio::sync::broadcast::error::TryRecvError;

/// The event bus itself. Held by `CapabilitySpace` and
/// consulted by `boot`. Subscribers see all events published
/// after they subscribed (events emitted before subscription
/// are lost — broadcast is not a replay log).
#[derive(Clone)]
pub struct GraphEventBus {
    tx: tokio::sync::broadcast::Sender<GraphEvent>,
}

impl GraphEventBus {
    /// Construct a bus with the default capacity (256).
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    /// Construct a bus with an explicit capacity. Smaller
    /// capacities let tests verify overflow handling without
    /// sending thousands of messages.
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(capacity.max(1));
        Self { tx }
    }

    /// Subscribe to the bus. The receiver sees events
    /// published **after** this call returns; earlier events
    /// are gone.
    ///
    /// Multiple subscribers are supported; each gets an
    /// independent view. Subscribers do not see each other's
    /// lag state.
    pub fn subscribe(&self) -> GraphEventReceiver {
        self.tx.subscribe()
    }

    /// Publish an event. Non-blocking. Returns `Ok(receiver_count)`
    /// if at least one receiver got it; `Err(ev)` if no
    /// subscribers are connected (the event is dropped). The
    /// drop is intentional: the bus never blocks a mutation,
    /// and "no subscribers" is the common case in tests.
    pub fn publish(&self, ev: GraphEvent) -> Result<usize, GraphEvent> {
        self.tx.send(ev).map_err(|e| e.0)
    }

    /// Number of active receivers. Mostly useful for diagnostics.
    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for GraphEventBus {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for GraphEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphEventBus")
            .field("receiver_count", &self.tx.receiver_count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_with_no_subscribers_is_dropped() {
        let bus = GraphEventBus::new();
        let result = bus.publish(GraphEvent::ShutdownStarted);
        // No subscribers: the event is dropped, returns the
        // event back via Err.
        assert!(result.is_err(), "no-receiver publish must drop");
    }

    #[test]
    fn subscribe_then_publish_delivers() {
        let bus = GraphEventBus::new();
        let mut rx = bus.subscribe();
        assert_eq!(bus.receiver_count(), 1);

        bus.publish(GraphEvent::ShutdownStarted).unwrap();
        let ev = rx.try_recv().expect("event delivered");
        assert_eq!(ev, GraphEvent::ShutdownStarted);
    }

    #[test]
    fn multiple_subscribers_each_get_event() {
        let bus = GraphEventBus::new();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();
        assert_eq!(bus.receiver_count(), 2);

        bus.publish(GraphEvent::ShutdownStarted).unwrap();
        assert_eq!(
            rx1.try_recv().unwrap(),
            GraphEvent::ShutdownStarted
        );
        assert_eq!(
            rx2.try_recv().unwrap(),
            GraphEvent::ShutdownStarted
        );
    }

    #[test]
    fn event_ordering_with_single_sender() {
        // `tokio::sync::broadcast` guarantees that a single
        // sender's events arrive at receivers in publish
        // order. Pin this in a test so any future migration
        // off broadcast surfaces immediately.
        let bus = GraphEventBus::new();
        let mut rx = bus.subscribe();

        bus.publish(GraphEvent::ShutdownStarted).unwrap();
        bus.publish(GraphEvent::ShutdownCompleted { remaining_slots: 0 })
            .unwrap();

        let first = rx.try_recv().unwrap();
        let second = rx.try_recv().unwrap();
        assert!(matches!(first, GraphEvent::ShutdownStarted));
        assert!(matches!(second, GraphEvent::ShutdownCompleted { .. }));
    }
}