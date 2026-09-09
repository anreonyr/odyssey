//! Typed `Capability<R>` and the erased `AnyCapability` view.

use std::any::Any;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use tokio::sync::mpsc;

use super::resource::Resource;
use super::types::{CapabilityChunk, CapabilityId, CapabilityMeta, CapabilityRights, CapKind};

// ---------------------------------------------------------------------------
// Capability<R> — typed capability handle
// ---------------------------------------------------------------------------

/// Unforgeable capability object. Wraps an `Arc<R>` (the resource) with
/// metadata, budget, and a runtime `CapKind`.
pub struct Capability<R: Resource> {
    meta: CapabilityMeta,
    handler: Arc<R>,
    budget: Arc<super::types::CapabilityBudget>,
    kind: CapKind,
}

impl<R: Resource> Capability<R> {
    pub(crate) fn new(
        meta: CapabilityMeta,
        handler: Arc<R>,
        budget: Arc<super::types::CapabilityBudget>,
        kind: CapKind,
    ) -> Self {
        Self { meta, handler, budget, kind }
    }

    pub fn name(&self) -> &str {
        &self.meta.name
    }
    pub fn id(&self) -> CapabilityId {
        self.meta.id.clone()
    }
    #[allow(dead_code)]
    pub fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }
    #[allow(dead_code)]
    pub fn kind(&self) -> CapKind {
        self.kind
    }
    pub fn rights(&self) -> CapabilityRights {
        CapabilityRights { timeout_ms: self.budget.timeout_ms }
    }

    /// Derive a new capability sharing the same handler `Arc<R>` with the
    /// source, with the given rights and a fresh `CapabilityId`. Kind is
    /// preserved.
    pub fn derive(&self, rights: CapabilityRights, new_id: CapabilityId) -> Self {
        let new_budget = Arc::new(super::types::CapabilityBudget::new(rights.timeout_ms));
        let mut new_meta = self.meta.clone();
        new_meta.id = new_id;
        new_meta.timeout_ms = rights.timeout_ms;
        Self {
            meta: new_meta,
            handler: self.handler.clone(),
            budget: new_budget,
            kind: self.kind,
        }
    }
}

impl<R: Resource> Clone for Capability<R> {
    fn clone(&self) -> Self {
        Self {
            meta: self.meta.clone(),
            handler: self.handler.clone(),
            budget: self.budget.clone(),
            kind: self.kind,
        }
    }
}

impl<R: Resource> Capability<R> {
    /// Invoke the resource. If elapsed wall-clock time exceeds the
    /// token's `timeout_ms`, returns `Err` and the handler result is
    /// dropped.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        if self.kind != CapKind::Sync {
            return Err(format!(
                "{}: not a sync capability",
                self.meta.name
            ));
        }
        let start = Instant::now();
        let result = self.handler.invoke(input);
        let elapsed_ms = start.elapsed().as_millis() as u64;
        if elapsed_ms > self.budget.timeout_ms as u64 {
            return Err(format!(
                "{}: timeout {}ms exceeded budget {}ms",
                self.meta.name, elapsed_ms, self.budget.timeout_ms
            ));
        }
        result
    }

    /// Open the stream. Per-chunk delivery is the resource's job; the
    /// budget governs the open-to-last-chunk window for the caller.
    pub fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        if self.kind != CapKind::Stream {
            return Err(format!(
                "{}: not a streaming capability",
                self.meta.name
            ));
        }
        self.handler.open(input)
    }
}

// ---------------------------------------------------------------------------
// AnyCapability — erased view, downcastable to Capability<R>
// ---------------------------------------------------------------------------

/// Erased capability: lets heterogeneous `Capability<R>` values coexist in
/// a single registry. Provides `as_any` for downcasting to a typed
/// `&dyn Any` reference; the caller can then re-`Arc::new` the cloned
/// capability (which is `Clone`).
pub trait AnyCapability: Any + Send + Sync {
    fn meta(&self) -> &CapabilityMeta;
    fn is_streaming(&self) -> bool;
    fn invoke_dyn(&self, input: Value) -> Result<Value, String>;
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String>;
    fn as_any(&self) -> &dyn Any;
}

impl<R: Resource> AnyCapability for Capability<R> {
    fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }
    fn is_streaming(&self) -> bool {
        self.kind == CapKind::Stream
    }
    fn invoke_dyn(&self, input: Value) -> Result<Value, String> {
        self.invoke(input)
    }
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        self.open(input)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}