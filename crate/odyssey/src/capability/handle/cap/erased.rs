//! `AnyCapability` — the erased view that lets heterogeneous
//! `Capability<R>` values coexist in one `CapabilitySpace`.
//!
//! Phase 5 fixes embedded in this file:
//!
//! - **M2** (`set_revoked_dyn` default): the default implementation
//!   is now `panic!`. A future `AnyCapability` impl that forgets to
//!   override this method fails loudly at the first revoke, instead
//!   of silently continuing to dispatch past revocation. The kernel's
//!   unforgeable-capability invariant requires every cap type to
//!   expose a marker that can be flipped on revoke; making this a
//!   panic is the right default — silent fall-through was the
//!   original soundness gap.

use std::any::Any;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::capability::error::CapabilityError;
use crate::capability::handle::cap::Capability;
use crate::core::contract::resource::Resource;
use crate::core::identity::kind::CapKind;
use crate::core::meta::chunk::CapabilityChunk;
use crate::core::meta::meta::CapabilityMeta;
use crate::core::rights::rights::Rights;

/// Erased capability: lets heterogeneous `Capability<R>` values
/// coexist in a single registry. Provides `as_any` for downcasting
/// to a typed `&dyn Any` reference.
pub trait AnyCapability: Any + Send + Sync {
    fn meta(&self) -> &CapabilityMeta;
    fn is_streaming(&self) -> bool;
    fn operations(&self) -> Rights;
    fn invoke_dyn(&self, op: Rights, input: Value) -> Result<Value, String>;
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String>;
    fn as_any(&self) -> &dyn Any;

    /// Sync invoke that returns the typed `CapabilityError`. The
    /// default impl wraps `invoke_dyn`'s string error via
    /// `CapabilityError::from_string_lossy`, but the typed
    /// `Capability<R>` impl returns the real typed variant so
    /// callers (the host pipeline, the agent dispatch) can
    /// pattern-match on `Revoked(SlotId)` / `SlotEmpty(SlotId)`
    /// without resorting to substring matching on `Display`.
    ///
    /// Phase 5 M4: the only consumer that needed typed matching
    /// was the pipeline. Now that the kernel exposes the typed
    /// variant here, the pipeline can stop doing substring
    /// checks against the rendered error message.
    ///
    /// Pre-M3: this method pair (`invoke_dyn` / `invoke_dyn_typed`)
    /// silently called the no-rights `Capability::invoke` path —
    /// the entire erased view bypassed the rights check. M3
    /// collapses the two views into one: every caller must
    /// declare the operation it needs, and the kernel enforces.
    fn invoke_dyn_typed(
        &self,
        op: Rights,
        input: Value,
    ) -> Result<Value, CapabilityError> {
        self.invoke_dyn(op, input)
            .map_err(|message| CapabilityError::Handler {
                name: self.meta().name.clone(),
                message,
            })
    }

    /// Revocable marker. Phase 5 M2: default impl panics. Every
    /// `AnyCapability` impl MUST override this to flip a marker
    /// that the dispatch path checks — otherwise the capability
    /// continues to dispatch past revocation, defeating the
    /// unforgeable-capability invariant.
    ///
    /// Phase 7 naming audit: the previous name was
    /// `mark_revoked_dyn`; the unified convention is
    /// `set_revoked(bool)` so install and revoke share a single
    /// entry point. The bool argument is `true` for "mark
    /// revoked" and `false` for "reset to live". The default
    /// panicked for both — silent fall-through was the original
    /// soundness gap — so the unified default still panics.
    fn set_revoked_dyn(&self, _revoked: bool) {
        panic!(
            "AnyCapability impl for `{}` did not override set_revoked_dyn; \
             caps of this type will continue to dispatch past revocation. \
             Implement the marker flip in the impl block.",
            self.meta().name
        );
    }
}

impl<R: Resource> AnyCapability for Capability<R> {
    fn meta(&self) -> &CapabilityMeta {
        // Use the public accessor; the field is private in Phase 5.
        // The method returns the same `&CapabilityMeta`.
        Capability::meta(self)
    }
    fn is_streaming(&self) -> bool {
        self.kind() == CapKind::Stream
    }
    fn operations(&self) -> Rights {
        Capability::operations(self)
    }
    fn invoke_dyn(&self, op: Rights, input: Value) -> Result<Value, String> {
        self.invoke(op, input).map_err(|e| e.to_string())
    }
    fn invoke_dyn_typed(
        &self,
        op: Rights,
        input: Value,
    ) -> Result<Value, CapabilityError> {
        self.invoke(op, input)
    }
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        self.open(input).map_err(|e| e.to_string())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn set_revoked_dyn(&self, revoked: bool) {
        self.set_revoked(revoked);
    }
}
