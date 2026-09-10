//! `Resource` trait — the abstraction plugin handlers implement.
//!
//! Phase 5: split from `capability::resource`. The trait is the
//! kernel-side shape; plugin authors implement it.

use serde_json::Value;
use tokio::sync::mpsc;

use crate::kernel::chunk::CapabilityChunk;

/// The kernel-side shape of a resource. Plugin handlers implement
/// one or both of `invoke` / `open`; default impls return
/// `CapabilityError::KindMismatch` so the wrong call shape fails
/// closed instead of silently no-op'ing.
pub trait Resource: Send + Sync + 'static {
    /// Default: sync invocation returns the typed
    /// `CapabilityError::KindMismatch`. Sync resources override.
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let _ = input;
        Err("resource does not support sync invocation".to_string())
    }

    /// Default: stream open returns the typed
    /// `CapabilityError::KindMismatch`. Stream resources override.
    fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let _ = input;
        Err("resource does not support streaming".to_string())
    }
}
