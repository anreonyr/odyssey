//! Personality-side observability vocabulary.
//!
//! Phase 8 split: kernel-side events (`Minted`, `Derived`,
//! `Revoked`, `RevokeTree`) live in `CapabilityEvent` and flow
//! through the kernel's `CapabilityEventBus`. Personality-side
//! events (`PluginDeactivated`, `ShutdownStarted`,
//! `ShutdownCompleted`) live here — the kernel has no knowledge
//! of plugins or shutdown sequence.
//!
//! Phase 9 cleanup: `LifecycleEvent::PluginActivated` is
//! removed (defined but never published; the boot path went
//! straight from mint to serve without an explicit "activated"
//! marker). The `subscribe` and `receiver_count` methods on
//! `LifecycleEventBus` are also gone — the bus is write-only
//! in the current orchestrator. If a future subscriber wants
//! to observe lifecycle events, add the reader back at the
//! same time the subscriber lands.

use crate::core::identity::ids::PluginId;

/// Personality lifecycle observability events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleEvent {
    PluginDeactivated { plugin: PluginId },
    ShutdownStarted,
    ShutdownCompleted { remaining_slots: usize },
}

/// Broadcast bus for personality lifecycle events. Default
/// capacity 256 covers any plausible boot/teardown sequence.
#[derive(Clone)]
pub struct LifecycleEventBus {
    tx: tokio::sync::broadcast::Sender<LifecycleEvent>,
}

impl LifecycleEventBus {
    pub fn new() -> Self {
        Self::with_capacity(256)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(capacity.max(1));
        Self { tx }
    }

    pub fn publish(&self, ev: LifecycleEvent) -> Result<usize, LifecycleEvent> {
        self.tx.send(ev).map_err(|e| e.0)
    }
}

impl Default for LifecycleEventBus {
    fn default() -> Self {
        Self::new()
    }
}
