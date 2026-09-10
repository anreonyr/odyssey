//! Graph event bus — broadcast channel for cspace mutations.
//!
//! Phase 5: moved from `capability::events` to `kernel::space::events`
//! (events are an observability view of the cspace, not a separate
//! "model" type). `PluginId` is now `crate::kernel::ids::PluginId`.

use crate::kernel::ids::PluginId;
use crate::kernel::ids::SlotId;

/// Default channel capacity. 256 events covers any plausible
/// boot or teardown sequence (we emit ≤ 4 events per plugin
/// + 2 shutdown markers, so 64 plugins fit comfortably).
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

pub type GraphEventReceiver = tokio::sync::broadcast::Receiver<GraphEvent>;
pub type TryRecvError = tokio::sync::broadcast::error::TryRecvError;

#[derive(Clone)]
pub struct GraphEventBus {
    tx: tokio::sync::broadcast::Sender<GraphEvent>,
}

impl GraphEventBus {
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(capacity.max(1));
        Self { tx }
    }

    pub fn subscribe(&self) -> GraphEventReceiver {
        self.tx.subscribe()
    }

    pub fn publish(&self, ev: GraphEvent) -> Result<usize, GraphEvent> {
        self.tx.send(ev).map_err(|e| e.0)
    }

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
