//! Typed `Capability<R>` and the erased `AnyCapability` view.

use std::any::Any;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use tokio::sync::mpsc;

use super::resource::Resource;
use super::types::{
    CapabilityChunk, CapabilityId, CapabilityMeta, CapabilityRights, CapKind, OperationRights,
};

// ---------------------------------------------------------------------------
// Capability<R> — typed capability handle
// ---------------------------------------------------------------------------

/// Unforgeable capability object. Wraps an `Arc<R>` (the resource) with
/// metadata, budget, and a runtime `CapKind`.
///
/// Beyond the per-call wall-clock budget, `Capability` also carries the
/// `OperationRights` it was minted (or last restricted) with. `invoke_op`
/// is the kernel-level guard that turns those bits into runtime
/// authority: every call site must declare which operation it is
/// performing and the capability rejects anything it doesn't hold.
pub struct Capability<R: Resource> {
    meta: CapabilityMeta,
    handler: Arc<R>,
    budget: Arc<super::types::CapabilityBudget>,
    /// Operations this cap was derived with. The kernel writes this
    /// at mint time and clamps it at every `restrict`.
    operations: OperationRights,
    kind: CapKind,
}

impl<R: Resource> Capability<R> {
    pub(crate) fn new(
        meta: CapabilityMeta,
        handler: Arc<R>,
        budget: Arc<super::types::CapabilityBudget>,
        kind: CapKind,
    ) -> Self {
        Self {
            meta,
            handler,
            budget,
            operations: OperationRights::ALL,
            kind,
        }
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

    /// Operations currently held. Can only be a subset of the parent's.
    pub fn operations(&self) -> OperationRights {
        self.operations
    }

    /// Full rights bag — both axes.
    pub fn rights(&self) -> CapabilityRights {
        CapabilityRights {
            operations: self.operations,
            timeout_ms: self.budget.timeout_ms,
        }
    }

    /// Derive a new capability sharing the same handler `Arc<R>` with the
    /// source, with the given rights and a fresh `CapabilityId`. Kind is
    /// preserved. The CSpace is responsible for verifying the requested
    /// rights are a subset of `self.operations` *before* calling derive;
    /// derive itself just records what the CSpace asked for.
    pub fn derive(&self, rights: CapabilityRights, new_id: CapabilityId) -> Self {
        let new_budget = Arc::new(super::types::CapabilityBudget::new(rights.timeout_ms));
        let mut new_meta = self.meta.clone();
        new_meta.id = new_id;
        new_meta.timeout_ms = rights.timeout_ms;
        Self {
            meta: new_meta,
            handler: self.handler.clone(),
            budget: new_budget,
            operations: rights.operations,
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
            operations: self.operations,
            kind: self.kind,
        }
    }
}

impl<R: Resource> Capability<R> {
    /// Invoke the resource **as if the caller held `EXECUTE`** — for
    /// backward compatibility with existing plugins that don't yet know
    /// about per-operation rights. New callers should use `invoke_op`
    /// and pick the right bit.
    ///
    /// Returns `Err` if the held rights don't include `EXECUTE`, the
    /// elapsed wall-clock time exceeds the token's `timeout_ms`, or
    /// the cap is a stream.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        self.invoke_op(OperationRights::EXECUTE, input)
    }

    /// The kernel-level guard: invoke `input` only if `self.operations`
    /// covers every bit in `requested`. The resource then runs under
    /// the same wall-clock budget enforcement as before.
    pub fn invoke_op(
        &self,
        requested: OperationRights,
        input: Value,
    ) -> Result<Value, String> {
        if self.kind != CapKind::Sync {
            return Err(format!("{}: not a sync capability", self.meta.name));
        }
        if !self.operations.contains(requested) {
            return Err(format!(
                "{}: operation denied — requested {:?}, held {:?}",
                self.meta.name, requested, self.operations
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
            return Err(format!("{}: not a streaming capability", self.meta.name));
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