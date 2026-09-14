//! Identity — CapabilityId, SlotId, PluginId, CapKind.
//!
//! Pure value types. No allocation beyond the inner integer /
//! string. No locking, no I/O. Stable across all kernel
//! boundaries (no kernel-internal state).

pub mod ids;
pub mod kind;
