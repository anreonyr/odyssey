//! `Resource` trait — the abstraction plugin handlers implement.
//!
//! Phase 5: split from `capability::resource`. The trait is the
//! kernel-side shape; plugin authors implement it.

use serde_json::Value;
use tokio::sync::mpsc;

use crate::kernel::chunk::CapabilityChunk;

/// The kernel-side shape of a resource. Plugin handlers implement
/// one or both of `invoke` / `open`; default impls return
/// `Err(\"resource does not support sync invocation\".to_string())`
/// (for `invoke`) or `Err(\"resource does not support streaming\".to_string())`
/// (for `open`) — these `String` errors are wrapped by the
/// dispatching `Capability::invoke` / `open` as
/// `CapabilityError::Handler` at the typed boundary, so the
/// wrong call shape fails closed instead of silently no-op'ing.
/// (Note: `CapabilityError::KindMismatch` is produced earlier,
/// at the sync/stream `CapKind` check in `Capability::invoke` /
/// `open`; the handler's own `String` error is wrapped as
/// `Handler` after the kind check passes.)
pub trait Resource: Send + Sync + 'static {
    /// Default: sync invocation returns an `Err(String)`;
    /// `Capability::invoke` wraps this as
    /// `CapabilityError::Handler { name, message }` (the
    /// `KindMismatch` variant is produced at the CapKind
    /// check, before the handler runs). Sync resources
    /// override.
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let _ = input;
        Err("resource does not support sync invocation".to_string())
    }

    /// Default: stream open returns an `Err(String)`;
    /// `Capability::open` wraps this as
    /// `CapabilityError::Handler { name, message }` (the
    /// `KindMismatch` variant is produced at the CapKind
    /// check, before the handler runs). Stream resources
    /// override.
    fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let _ = input;
        Err("resource does not support streaming".to_string())
    }
}
