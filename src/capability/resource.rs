//! Resource trait — the body of a capability.
//!
//! A `Resource` provides both `invoke` (for sync caps) and `open` (for
//! streaming caps). The default impls return `Err` so a plugin only
//! needs to override the one that applies — sync plugins override
//! `invoke`, stream plugins override `open`.
//!
//! `Capability<R>::invoke` and `Capability<R>::open` both runtime-check
//! the cap's `CapKind` before calling the handler, so the wrong
//! override can never be reached.

use serde_json::Value;
use tokio::sync::mpsc;

use super::types::CapabilityChunk;

pub trait Resource: Send + Sync + 'static {
    fn invoke(&self, _input: Value) -> Result<Value, String> {
        Err("not a sync capability".into())
    }
    fn open(&self, _input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        Err("not a streaming capability".into())
    }
}