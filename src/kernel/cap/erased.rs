//! `AnyCapability` — the erased view that lets heterogeneous
//! `Capability<R>` values coexist in one `CapabilitySpace`.
//!
//! Phase 5 fixes embedded in this file:
//!
//! - **M2** (`mark_revoked_dyn` default): the default implementation
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

use crate::kernel::cap::Capability;
use crate::kernel::chunk::CapabilityChunk;
use crate::kernel::error::CapabilityError;
use crate::kernel::kind::CapKind;
use crate::kernel::meta::CapabilityMeta;
use crate::kernel::resource::Resource;
use crate::kernel::rights::OperationRights;

/// Erased capability: lets heterogeneous `Capability<R>` values
/// coexist in a single registry. Provides `as_any` for downcasting
/// to a typed `&dyn Any` reference.
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
    fn invoke_dyn_typed(&self, input: Value) -> Result<Value, CapabilityError> {
        self.invoke_dyn(input)
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
    fn mark_revoked_dyn(&self) {
        panic!(
            "AnyCapability impl for `{}` did not override mark_revoked_dyn; \
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
    fn operations(&self) -> OperationRights {
        Capability::operations(self)
    }
    fn invoke_dyn(&self, input: Value) -> Result<Value, String> {
        self.invoke(input).map_err(|e| e.to_string())
    }
    fn invoke_op_dyn(
        &self,
        op: OperationRights,
        input: Value,
    ) -> Result<Value, String> {
        self.invoke_op(op, input).map_err(|e| e.to_string())
    }
    fn invoke_dyn_typed(&self, input: Value) -> Result<Value, CapabilityError> {
        self.invoke(input)
    }
    fn open_dyn(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        self.open(input).map_err(|e| e.to_string())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn mark_revoked_dyn(&self) {
        self.mark_revoked();
    }
}
