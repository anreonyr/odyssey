//! Stream chunk type.

use serde_json::Value;

/// One chunk in a streaming capability response.
///
/// Phase 2 keeps the enum shape unchanged — cost accounting happens
/// at the *capability* layer (the kernel credits the parent quota
/// bucket), not inside the chunk. Resources that want their output
/// counted report the per-chunk token / byte cost through the
/// `Capability::open_with_quota` API; for the simple `open` path the
/// cost is `1 token / item`.
#[derive(Debug)]
pub enum CapabilityChunk<T = Value> {
    Item(T),
    Done,
}
