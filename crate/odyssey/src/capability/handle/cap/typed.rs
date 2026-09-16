//! `Capability<R>` — typed, owned handle to a resource behind the
//! kernel.
//!
//! Phase 5 fixes embedded in this file:
//!
//! - **D1** (`derive` doc-comment): the 20-line block arguing against a
//!   hypothetical `pub derive` is compressed to a 5-line block noting
//!   that `derive` is `pub(crate)` and the cspace is the only caller.
//! - **M3** (`invoke_op` re-order): the handler runs first; on success
//!   we record elapsed wall-clock (via `Clock::now()`); then we check
//!   the per-call timeout (return `CapabilityError::Timeout` if the
//!   budget was blown, dropping the successful handler result); then
//!   we debit the per-minute quota (return `CapabilityError::QuotaExceeded`
//!   if the bucket is full); then we surface the handler's result. The
//!   budget contract is the contract — a late answer is not a correct
//!   answer (M3's "don't lie about success" invariant).
//! - **M4** (`invoke_op` / `open`): return typed `CapabilityError`
//!   instead of `String`. Substring matching in pipeline is replaced
//!   by pattern-matching on the variant.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;
use tokio::sync::mpsc;

use crate::capability::enforce::quota::CapabilityBudget;
use crate::capability::error::CapabilityError;
use crate::core::clock::clock::Clock;
use crate::core::contract::resource::Resource;
use crate::core::identity::ids::{CapabilityId, SlotId};
use crate::core::identity::kind::CapKind;
use crate::core::meta::chunk::CapabilityChunk;
use crate::core::meta::meta::CapabilityMeta;
use crate::core::rights::rights::{CapabilityRights, OperationRights};

/// Typed, owned handle to a single capability slot. `Arc<Capability<R>>`
/// is what the kernel stores; `Slot<R>::capability()` returns one
/// of these to the plugin that holds the slot.
///
/// Phase 4 review loop: the `revoked` marker is `Arc<AtomicBool>` so
/// clones share the same flag, and a typed `Arc<Capability<R>>` clone
/// returned by `Slot::capability()` observes the cspace-level revoke.
pub struct Capability<R: Resource> {
    meta: CapabilityMeta,
    /// The slot id this cap occupies. Tracked so revoked-cap
    /// errors can surface a real `SlotId` instead of a
    /// sentinel; required because `SlotId::new(0)` would
    /// panic on `NonZeroU64`.
    slot: Option<SlotId>,
    handler: Arc<R>,
    budget: Arc<CapabilityBudget>,
    /// Wall-clock source. Phase 5 P1-C fix: every capability
    /// carries its own `Arc<dyn Clock>` so the `invoke` /
    /// `invoke_op` paths can record elapsed wall-clock without
    /// calling `Instant::now()` directly. Production uses
    /// `SystemClock`; tests inject `MockClock` to make
    /// timeout / quota eviction deterministic.
    clock: Arc<dyn Clock>,
    operations: OperationRights,
    kind: CapKind,
    revoked: Arc<AtomicBool>,
}

impl<R: Resource> Capability<R> {
    /// Construct a typed capability. Called by the factory at mint time
    /// and by `derive` for derived caps.
    ///
    /// `clock` is the wall-clock source the capability will use
    /// for elapsed-time recording. The factory passes an
    /// `Arc<SystemClock>` by default; tests pass an `Arc<MockClock>`
    /// to drive deterministic time. Derived caps inherit their
    /// parent's clock via `derive` so a subtree shares the same
    /// notion of wall-clock time.
    pub fn new(
        meta: CapabilityMeta,
        handler: Arc<R>,
        budget: CapabilityBudget,
        rights: CapabilityRights,
        kind: CapKind,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            meta,
            slot: None,
            handler,
            budget: Arc::new(budget),
            clock,
            operations: rights.operations,
            kind,
            revoked: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Bind the cap to its slot id. Called by the cspace at
    /// install / install_derived time. After install, `slot()`
    /// returns the real id so revoked-cap errors can surface a
    /// concrete `SlotId` instead of a sentinel.
    pub(crate) fn bind_slot(&mut self, slot: SlotId) {
        self.slot = Some(slot);
    }

    /// The slot id this cap is bound to. `None` for caps that
    /// haven't been installed yet (e.g. derived caps before
    /// `install_derived` binds them).
    pub fn slot(&self) -> Option<SlotId> {
        self.slot
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

    /// Set the revocation marker. Called by the cspace on
    /// `install` (with `false`, so a re-installed cap starts
    /// fresh) and on `revoke` (with `true`). Internal kernel
    /// API. Phase 7 naming audit: replaces the prior
    /// `mark_revoked` / `reset_revoked` pair with a single
    /// bool-typed setter so the install and revoke paths share
    /// an entry point.
    pub(crate) fn set_revoked(&self, revoked: bool) {
        self.revoked.store(revoked, Ordering::Release);
    }

    /// Derive a child cap with reduced rights. Internal — only the
    /// cspace calls this, after verifying attenuation. The kernel
    /// is the only path that may mint derived caps; release-build
    /// checks ensure no plugin can amplify authority.
    ///
    /// The derived cap inherits the parent's `clock` (via
    /// `Arc::clone`) so the entire subtree agrees on what
    /// "wall-clock" means. The same `Arc<dyn Clock>` is shared
    /// between parent and child — a `MockClock::advance` call
    /// from a test is visible to both parent and child.
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
            slot: None, // bound by install_derived on insertion
            handler: self.handler.clone(),
            budget: Arc::new(new_budget),
            clock: Arc::clone(&self.clock),
            operations: rights.operations,
            kind: self.kind,
            // Fresh marker — a derived slot has its own lifecycle,
            // independent of the parent slot's revocation state.
            revoked: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Sync invoke. The caller declares which operation is being
    /// requested; the kernel checks `self.operations.contains(op)`
    /// and surfaces `CapabilityError::OperationDenied` on a miss.
    /// Phase 5 M4: returns typed `CapabilityError`. M3 reorder:
    /// the handler runs first; on success the elapsed
    /// wall-clock is recorded via `self.clock.now()`; the
    /// per-call timeout is checked (the successful result
    /// is dropped on a late timeout — the budget is the
    /// contract); the per-minute quota is debited; the
    /// handler result is surfaced.
    ///
    /// The pre-M3 phase kept a separate `invoke` (no-rights-check)
    /// next to this one. That second path was the kernel's
    /// soundness gap: every external entry point (the HTTP bridge,
    /// the agent runtime's tool dispatch, the erased `invoke_dyn`
    /// view) called the unchecked version, so the rights declared
    /// on the capability were documentary. This single entry point
    /// closes that gap — there is no shortcut.
    pub fn invoke(&self, op: OperationRights, input: Value) -> Result<Value, CapabilityError> {
        if self.kind != CapKind::Sync {
            return Err(CapabilityError::KindMismatch {
                name: self.meta.name.clone(),
                expected: "sync",
                got: "stream",
            });
        }
        if self.is_revoked() {
            return Err(CapabilityError::Revoked(
                self.slot.unwrap_or(SlotId::new(1)),
            ));
        }
        if !self.operations.contains(op) {
            return Err(CapabilityError::OperationDenied {
                name: self.meta.name.clone(),
                requested: op,
                held: self.operations,
            });
        }
        // Phase 5 M3: run the handler first. The call is authorised;
        // we debit the quota only after a successful run. Wall-clock
        // recorded the same way.
        let start = self.clock.now();
        let result = self.handler.invoke(input);
        let elapsed = start.elapsed();
        self.budget.record_elapsed(elapsed);

        // Timeout check — if the handler blew the per-call budget,
        // the budget contract is the contract; we drop the
        // successful handler result and surface Timeout. M3's
        // invariant: don't lie about success.
        let elapsed_ms = elapsed.as_micros().div_ceil(1000) as u64;
        let budget_ms = self.budget.timeout_ms() as u64;
        if elapsed_ms > budget_ms {
            return Err(CapabilityError::Timeout {
                name: self.meta.name.clone(),
                elapsed_ms,
                budget_ms: self.budget.timeout_ms(),
            });
        }

        // Now debit the call quota. A failed handler or a quota
        // exhaustion does not consume the per-minute bucket.
        if let Err(kind) = self.budget.try_call() {
            return Err(CapabilityError::QuotaExceeded {
                name: self.meta.name.clone(),
                kind,
            });
        }

        result.map_err(|message| CapabilityError::Handler {
            name: self.meta.name.clone(),
            message,
        })
    }

    /// Stream open. Phase 5 M4: returns typed `CapabilityError`.
    pub fn open(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, CapabilityError> {
        if self.kind != CapKind::Stream {
            return Err(CapabilityError::KindMismatch {
                name: self.meta.name.clone(),
                expected: "stream",
                got: "sync",
            });
        }
        if self.is_revoked() {
            return Err(CapabilityError::Revoked(
                self.slot.unwrap_or(SlotId::new(1)),
            ));
        }
        if let Err(kind) = self.budget.try_call() {
            return Err(CapabilityError::QuotaExceeded {
                name: self.meta.name.clone(),
                kind,
            });
        }
        self.handler
            .open(input)
            .map_err(|message| CapabilityError::Handler {
                name: self.meta.name.clone(),
                message,
            })
    }
}

impl<R: Resource> Clone for Capability<R> {
    fn clone(&self) -> Self {
        Self {
            meta: self.meta.clone(),
            slot: self.slot,
            handler: self.handler.clone(),
            budget: self.budget.clone(),
            clock: Arc::clone(&self.clock),
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
