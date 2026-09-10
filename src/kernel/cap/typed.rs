//! `Capability<R>` — typed, owned handle to a resource behind the
//! kernel.
//!
//! Phase 5 fixes embedded in this file:
//!
//! - **D1** (`derive` doc-comment): the 20-line block arguing against a
//!   hypothetical `pub derive` is compressed to a 5-line block noting
//!   that `derive` is `pub(crate)` and the cspace is the only caller.
//! - **M3** (`invoke_op` re-order): quota is debited BEFORE the handler
//!   runs (the call has been authorised). Wall-clock is recorded
//!   after the handler returns. The handler's success result is
//!   never silently discarded on a late timeout — `M3`'s "don't lie
//!   about success" invariant holds.
//! - **M4** (`invoke_op` / `open`): return typed `CapabilityError`
//!   instead of `String`. Substring matching in pipeline is replaced
//!   by pattern-matching on the variant.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::kernel::chunk::CapabilityChunk;
use crate::kernel::error::CapabilityError;
use crate::kernel::ids::{CapabilityId, SlotId};
use crate::kernel::kind::CapKind;
use crate::kernel::meta::CapabilityMeta;
use crate::kernel::quota::CapabilityBudget;
use crate::kernel::resource::Resource;
use crate::kernel::rights::{CapabilityRights, OperationRights};

/// Typed, owned handle to a single capability slot. `Arc<Capability<R>>`
/// is what the kernel stores; `Slot<R>::capability()` returns one
/// of these to the plugin that holds the slot.
///
/// Phase 4 review loop: the `revoked` marker is `Arc<AtomicBool>` so
/// clones share the same flag, and a typed `Arc<Capability<R>>` clone
/// returned by `Slot::capability()` observes the cspace-level revoke.
pub struct Capability<R: Resource> {
    meta: CapabilityMeta,
    handler: Arc<R>,
    budget: Arc<CapabilityBudget>,
    operations: OperationRights,
    kind: CapKind,
    revoked: Arc<AtomicBool>,
}

impl<R: Resource> Capability<R> {
    /// Construct a typed capability. Called by the factory at mint time
    /// and by `derive` for derived caps.
    pub fn new(
        meta: CapabilityMeta,
        handler: Arc<R>,
        budget: CapabilityBudget,
        rights: CapabilityRights,
        kind: CapKind,
    ) -> Self {
        Self {
            meta,
            handler,
            budget: Arc::new(budget),
            operations: rights.operations,
            kind,
            revoked: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_revoked(&self) -> bool {
        self.revoked.load(Ordering::Acquire)
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

    /// Borrow the budget for introspection (snapshot, timeout).
    pub fn budget(&self) -> &CapabilityBudget {
        &self.budget
    }

    /// Operations currently held. Can only be a subset of the parent's.
    pub fn operations(&self) -> OperationRights {
        self.operations
    }

    pub fn rights(&self) -> CapabilityRights {
        CapabilityRights {
            operations: self.operations,
            timeout_ms: self.budget.timeout_ms(),
        }
    }

    /// Reset the revocation marker. Called by the cspace on `install`
    /// so a re-installed cap starts fresh. Internal kernel API.
    pub(crate) fn reset_revoked(&self) {
        self.revoked.store(false, Ordering::Release);
    }

    /// Flip the revocation marker. Called by the cspace on `revoke`
    /// paths. Internal kernel API.
    pub(crate) fn mark_revoked(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    /// Derive a child cap with reduced rights. Internal — only the
    /// cspace calls this, after verifying attenuation. The kernel
    /// is the only path that may mint derived caps; release-build
    /// checks ensure no plugin can amplify authority.
    pub(crate) fn derive(&self, rights: CapabilityRights, new_id: CapabilityId) -> Self {
        // Phase 5 D1: kernel is the only caller; cspace has already
        // verified `held.contains(&rights)`. Tripwire remains so a
        // future caller that forgets the precondition panics in
        // debug builds.
        debug_assert!(
            self.operations.contains(rights.operations),
            "Capability::derive would amplify rights: held={:?}, requested={:?}",
            self.operations,
            rights.operations
        );
        let new_budget = CapabilityBudget::share_with(&self.budget, rights.timeout_ms);
        let mut new_meta = self.meta.clone();
        new_meta.id = new_id;
        new_meta.timeout_ms = rights.timeout_ms;
        Self {
            meta: new_meta,
            handler: self.handler.clone(),
            budget: Arc::new(new_budget),
            operations: rights.operations,
            kind: self.kind,
            // Fresh marker — a derived slot has its own lifecycle,
            // independent of the parent slot's revocation state.
            revoked: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Sync invoke without an operation-rights check. Internal — the
    /// public typed entry point is `Slot::invoke`. Returns `String`
    /// for backwards compatibility with the runtime plugin handlers
    /// that return `Result<Value, String>`.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        if self.kind != CapKind::Sync {
            return Err(format!("{}: not a sync capability", self.meta.name));
        }
        if self.is_revoked() {
            return Err(format!("{}: capability revoked", self.meta.name));
        }
        if let Err(kind) = self.budget.try_call() {
            return Err(format!(
                "{}: quota exhausted ({}); snapshot={:?}",
                self.meta.name,
                kind,
                self.budget.snapshot()
            ));
        }
        let start = Instant::now();
        let result = self.handler.invoke(input);
        self.budget.record_elapsed(start.elapsed());
        result
    }

    /// Sync invoke with operation-rights check. Phase 5 M4: returns
    /// typed `CapabilityError`. The handler's `String` error is
    /// wrapped as `CapabilityError::Handler`.
    pub fn invoke_op(
        &self,
        requested: OperationRights,
        input: Value,
    ) -> Result<Value, CapabilityError> {
        if self.kind != CapKind::Sync {
            return Err(CapabilityError::KindMismatch {
                name: self.meta.name.clone(),
                expected: "sync",
                got: "stream",
            });
        }
        if self.is_revoked() {
            return Err(CapabilityError::SlotEmpty(SlotId::new(0)));
        }
        if !self.operations.contains(requested) {
            return Err(CapabilityError::OperationDenied {
                name: self.meta.name.clone(),
                requested,
                held: self.operations,
            });
        }
        // Phase 5 M3: quota debited before handler runs (the call is
        // authorised). Wall-clock recorded after.
        if let Err(kind) = self.budget.try_call() {
            return Err(CapabilityError::QuotaExceeded {
                name: self.meta.name.clone(),
                kind,
            });
        }
        let start = Instant::now();
        let result = self.handler.invoke(input);
        self.budget.record_elapsed(start.elapsed());
        result.map_err(|message| CapabilityError::Handler {
            name: self.meta.name.clone(),
            message,
        })
    }

    /// Stream open. Phase 5 M4: returns typed `CapabilityError`.
    pub fn open(
        &self,
        input: Value,
    ) -> Result<mpsc::Receiver<CapabilityChunk>, CapabilityError> {
        if self.kind != CapKind::Stream {
            return Err(CapabilityError::KindMismatch {
                name: self.meta.name.clone(),
                expected: "stream",
                got: "sync",
            });
        }
        if self.is_revoked() {
            return Err(CapabilityError::SlotEmpty(SlotId::new(0)));
        }
        if let Err(kind) = self.budget.try_call() {
            return Err(CapabilityError::QuotaExceeded {
                name: self.meta.name.clone(),
                kind,
            });
        }
        self.handler.open(input).map_err(|message| {
            CapabilityError::Handler {
                name: self.meta.name.clone(),
                message,
            }
        })
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
            // the `cspace::revoke` flip.
            revoked: Arc::clone(&self.revoked),
        }
    }
}
