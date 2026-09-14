//! Personality-side observability vocabulary.
//!
//! Phase 8 split: kernel-side events (`Minted`, `Derived`,
//! `Revoked`, `RevokeTree`) live in `CapabilityEvent` and flow
//! through the kernel's `CapabilityEventBus`. Personality-side
//! events (`PluginActivated`, `PluginDeactivated`,
//! `ShutdownStarted`, `ShutdownCompleted`) live here — the
//! kernel has no knowledge of plugins or shutdown sequence.

use crate::core::identity::ids::PluginId;

/// Personality lifecycle observability events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleEvent {
    PluginActivated { plugin: PluginId },
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

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<LifecycleEvent> {
        self.tx.subscribe()
    }

    pub fn publish(&self, ev: LifecycleEvent) -> Result<usize, LifecycleEvent> {
        self.tx.send(ev).map_err(|e| e.0)
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for LifecycleEventBus {
    fn default() -> Self {
        Self::new()
    }
}
