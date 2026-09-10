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
    /// Revocable marker (Phase 4 P4.8 review loop). When set,
    /// `invoke` and `invoke_op` refuse to dispatch — even if
    /// some holder kept an `Arc<Capability<R>>` clone past
    /// the cspace-level revocation. The kernel (`cspace::revoke`)
    /// flips this to `true` just before tearing down the slot;
    /// `cspace::install` resets it to `false` because a freshly
    /// installed slot has its own lifecycle.
    ///
    /// Wrapped in `Arc` so that every clone of the same
    /// `Capability<R>` shares the same atomic — the broken
    /// typed-cap path is fixed by this single change
    /// (`Capability::clone` does `Arc::clone(&self.revoked)`).
    ///
    /// This is best-effort: if a holder clones the `Arc<R>`
    /// handler directly (bypassing the cap wrapper), they can
    /// still call into the resource. The marker is the
    /// kernel-level guard for the typed-cap path, which is
    /// the only path the agent uses.
    revoked: std::sync::Arc<std::sync::atomic::AtomicBool>,
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
            revoked: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Reset the revoked flag. Called by `cspace::install`
    /// when a freshly-allocated slot gets a new cap — the
    /// cap's own lifecycle begins at install time, regardless
    /// of any past state the same `Arc<R>` handler might have
    /// shared. Only accessible from the kernel.
    pub(crate) fn reset_revoked(&self) {
        self.revoked.store(false, std::sync::atomic::Ordering::Release);
    }

    /// Mark the cap as revoked. Called by `cspace::revoke`
    /// just before the slot is torn down. The Arc is kept
    /// alive in case any holder wants to observe the
    /// post-revoke state; further invocations return Err.
    pub(crate) fn mark_revoked(&self) {
        self.revoked.store(true, std::sync::atomic::Ordering::Release);
    }

    /// True if the cap has been revoked. Primarily for tests
    /// and diagnostics — `invoke` already enforces this
    /// internally.
    #[allow(dead_code)]
    pub fn is_revoked(&self) -> bool {
        self.revoked.load(std::sync::atomic::Ordering::Acquire)
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

    /// Derive a new capability sharing the same handler `Arc<R>` and the
    /// same `Arc<QuotaState>` (so a child's calls debit the parent's
    /// rate-limit bucket) with the given rights and a fresh
    /// `CapabilityId`. Kind is preserved. The CSpace is responsible for
    /// verifying the requested rights are a subset of `self.operations`
    /// *before* calling derive; derive itself just records what the
    /// CSpace asked for.
    ///
    /// Quota sharing is the attenuation semantics the doc on
    /// `CapabilityBudget` promises: a derived cap cannot mint extra
    /// calls beyond the parent's bucket. To get per-leaf accounting,
    /// mint a fresh capability via `CapabilityFactory::mint`.
    pub(crate) fn derive(&self, rights: CapabilityRights, new_id: CapabilityId) -> Self {
        // P0-b (Phase 4 review loop): the doc-comment promises
        // "The CSpace is responsible for verifying the requested
        // rights are a subset of `self.operations` *before* calling
        // derive; derive itself just records what the CSpace asked
        // for." That contract is fine when the only caller is the
        // CSpace, but `derive` is `pub` (not `pub(crate)`) and the
        // capability kernel is reachable from anywhere with a
        // `CapabilitySpace`. A plugin with a `Capability<R>` in hand
        // could mint a full-rights child of a READ-only parent,
        // amplifying authority at the type level and bypassing the
        // attenuation invariant the CSpace is supposed to enforce.
        //
        // The fix: a `debug_assert!` at the kernel. Release builds
        // stay zero-cost; debug builds (which is what `cargo test`
        // runs) catch the bug immediately. All current CSpace
        // callers (`grant`, `transfer`, `restrict`) already verify
        // `held.contains(&rights)` before calling `derive`, so
        // this assert is a no-op for the happy path. New external
        // callers that forget the precondition panic in debug.
        debug_assert!(
            self.operations.contains(rights.operations),
            "Capability::derive would amplify rights: held={:?}, requested={:?}",
            self.operations, rights.operations
        );
        let new_budget = Arc::new(super::types::CapabilityBudget::share_quota_with(
            &self.budget,
            rights.timeout_ms,
        ));
        let mut new_meta = self.meta.clone();
        new_meta.id = new_id;
        new_meta.timeout_ms = rights.timeout_ms;
        Self {
            meta: new_meta,
            handler: self.handler.clone(),
            budget: new_budget,
            operations: rights.operations,
            kind: self.kind,
            // Fresh marker — a derived slot has its own lifecycle,
            // independent of the parent slot's revocation state.
            revoked: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
            // Revocable marker (Phase 4 review loop): share
            // the Arc so the typed-cap path (`Slot::capability()`
            // → `Arc::new(concrete.clone())`) actually sees
            // the `cspace::revoke` flip. The kernel keeps only
            // one canonical `Capability<R>` per slot, but
            // every clone must share the flag, otherwise
            // cached typed `Arc<Capability<R>>`s survive
            // revocation invisibly.
            revoked: std::sync::Arc::clone(&self.revoked),
        }
    }
}

impl<R: Resource> Capability<R> {
    /// Invoke the resource **declaring `EXECUTE`** as the caller's
    /// bit. This is the path plugin code uses — the resource handler
    /// itself stays bit-agnostic, and `EXECUTE` is the conventional
    /// bit for "I want this thing to do its thing".
    ///
    /// Code that needs to exercise a *specific* bit (the policy
    /// layer: `RuleAgent`, `Pipeline`, anything type-erased that has
    /// to introspect `cap.operations()` first) uses `invoke_op(bit,
    /// input)` instead, so the kernel guard sees the bit the caller
    /// is actually claiming.
    ///
    /// Returns `Err` if the held rights don't include `EXECUTE`, the
    /// elapsed wall-clock time exceeds the token's `timeout_ms`, the
    /// rate-limit quota (`calls_per_minute`) is exhausted, or the cap
    /// is a stream.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        self.invoke_op(OperationRights::EXECUTE, input)
    }

    /// The kernel-level guard: invoke `input` only if `self.operations`
    /// covers every bit in `requested`. The resource then runs under
    /// the same wall-clock budget enforcement as before, plus the
    /// rate-limit quota check at the head.
    pub fn invoke_op(
        &self,
        requested: OperationRights,
        input: Value,
    ) -> Result<Value, String> {
        // Revocable marker (Phase 4 review loop): the kernel
        // sets this in `cspace::revoke` before tearing down
        // the slot. Even if a holder kept an `Arc<Capability<R>>`
        // clone past revocation (e.g. an agent that captured
        // the cap before the parent slot was revoked), the
        // kernel-level guard refuses to dispatch.
        if self.revoked.load(std::sync::atomic::Ordering::Acquire) {
            return Err(format!("{}: capability revoked", self.meta.name));
        }
        if self.kind != CapKind::Sync {
            return Err(format!("{}: not a sync capability", self.meta.name));
        }
        if !self.operations.contains(requested) {
            return Err(format!(
                "{}: operation denied — requested {:?}, held {:?}",
                self.meta.name, requested, self.operations
            ));
        }
        // Quota check at the kernel level. QuotaState is shared via
        // Arc with the parent cap, so this deducts from the parent
        // bucket — which is the correct attenuation semantics.
        if let Err(kind) = self.budget.quota_state.try_call() {
            return Err(format!(
                "{}: quota exhausted ({}); used this minute = {:?}",
                self.meta.name,
                kind,
                self.budget.quota_state.snapshot()
            ));
        }
        let start = Instant::now();
        let result = self.handler.invoke(input);
        let elapsed_ms = start.elapsed().as_millis() as u64;
        self.budget
            .wall_clock_total_ms
            .fetch_add(elapsed_ms, std::sync::atomic::Ordering::Relaxed);
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
    ///
    /// Phase 2: the call quota is debited on `open`; per-chunk token
    /// accounting happens at the resource layer (the resource's
    /// `open` returns the receiver and the caller pushes chunks;
    /// resources that want their output counted should report tokens
    /// via the meta or via a helper on the open path).
    pub fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        if self.kind != CapKind::Stream {
            return Err(format!("{}: not a streaming capability", self.meta.name));
        }
        if let Err(kind) = self.budget.quota_state.try_call() {
            return Err(format!(
                "{}: quota exhausted on open ({})",
                self.meta.name, kind
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
    fn operations(&self) -> OperationRights;
    fn invoke_dyn(&self, input: Value) -> Result<Value, String>;
    fn invoke_op_dyn(
        &self,
        op: OperationRights,
        input: Value,
    ) -> Result<Value, String>;
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String>;
    fn as_any(&self) -> &dyn Any;
    /// Revocable marker (Phase 4 review loop): flip the
    /// `revoked` flag so future `invoke`/`invoke_op` calls
    /// return Err. Default implementation is a no-op so
    /// any future `AnyCapability` impl can opt out; the
    /// production impl on `Capability<R>` does the actual
    /// atomic store.
    fn mark_revoked_dyn(&self) {}
}

impl<R: Resource> AnyCapability for Capability<R> {
    fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }
    fn is_streaming(&self) -> bool {
        self.kind == CapKind::Stream
    }
    fn operations(&self) -> OperationRights {
        self.operations
    }
    fn invoke_dyn(&self, input: Value) -> Result<Value, String> {
        self.invoke(input)
    }
    fn invoke_op_dyn(
        &self,
        op: OperationRights,
        input: Value,
    ) -> Result<Value, String> {
        self.invoke_op(op, input)
    }
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        self.open(input)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn mark_revoked_dyn(&self) {
        self.mark_revoked();
    }
}