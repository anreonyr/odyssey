//! `Slot<R>` — typed, unforgeable reference to a slot. The unit of
//! possession; plugins hold one of these; the CSpace can revoke the
//! slot's contents without invalidating the slot reference itself.

use std::marker::PhantomData;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::capability::handle::cap::Capability;
use crate::core::meta::chunk::CapabilityChunk;
use crate::capability::error::CapabilityError;
use crate::core::identity::ids::SlotId;
use crate::core::identity::kind::CapKind;
use crate::core::meta::meta::CapabilityMeta;
use crate::core::contract::resource::Resource;
use crate::core::rights::rights::{CapabilityRights, OperationRights};

/// Typed, unforgeable reference to a slot.
pub struct Slot<R: Resource> {
    space: crate::capability::enforce::space::CapabilitySpace,
    id: SlotId,
    _phantom: PhantomData<R>,
}

impl<R: Resource> Slot<R> {
    pub fn new(space: crate::capability::enforce::space::CapabilitySpace, id: SlotId) -> Self {
        Self {
            space,
            id,
            _phantom: PhantomData,
        }
    }

    pub fn id(&self) -> SlotId {
        self.id
    }

    /// The capability currently occupying this slot, if any. Returns
    /// `None` when the slot has been revoked or is empty.
    pub fn capability(&self) -> Option<Arc<Capability<R>>> {
        self.space.lookup_typed::<R>(self.id)
    }

    /// Capability metadata at this slot.
    pub fn meta(&self) -> Option<CapabilityMeta> {
        self.space.slot_meta(self.id)
    }

    /// Runtime kind of the cap at this slot.
    #[allow(dead_code)]
    pub fn kind(&self) -> Option<CapKind> {
        self.capability().map(|c| c.kind())
    }

    /// Direct sync invocation via the slot. Returns typed
    /// `CapabilityError` (Phase 5 M4 / n1).
    pub fn invoke(&self, input: Value) -> Result<Value, CapabilityError> {
        let cap = self
            .capability()
            .ok_or(CapabilityError::SlotEmpty(self.id))?;
        cap.invoke(input)
    }

    /// Operation-aware invocation via the slot. Resolves the slot
    /// then defers to `Capability::invoke_op`.
    pub fn invoke_op(
        &self,
        op: OperationRights,
        input: Value,
    ) -> Result<Value, CapabilityError> {
        let cap = self
            .capability()
            .ok_or(CapabilityError::SlotEmpty(self.id))?;
        cap.invoke_op(op, input)
    }

    /// Direct stream open via the slot. Phase 5 M4: typed
    /// `CapabilityError`.
    pub fn open(
        &self,
        input: Value,
    ) -> Result<mpsc::Receiver<CapabilityChunk>, CapabilityError> {
        let cap = self
            .capability()
            .ok_or(CapabilityError::SlotEmpty(self.id))?;
        cap.open(input)
    }

    /// **Grant**: derive a new slot with reduced rights; source preserved.
    /// seL4: CNode.Mint.
    pub fn grant(
        &self,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        self.space.grant::<R>(self.id, rights, new_name)
    }

    /// **Restrict**: same as `grant` with intent distinction — derive a
    /// more limited view of your own capability.
    pub fn restrict(
        &self,
        rights: CapabilityRights,
        new_name: String,
    ) -> Result<SlotId, CapabilityError> {
        self.space.restrict::<R>(self.id, rights, new_name)
    }

    /// **Transfer**: move the capability to a fresh slot. Source cleared.
    /// seL4: CNode.Move.
    pub fn transfer(&self, rights: CapabilityRights) -> Result<SlotId, CapabilityError> {
        self.space.transfer::<R>(self.id, rights)
    }

    /// **Revoke**: clear the slot this handle points at.
    pub fn revoke(&self) -> bool {
        self.space.revoke(self.id)
    }

    /// **Revoke tree**: clear this slot and every descendant.
    pub fn revoke_tree(&self) -> usize {
        self.space.revoke_tree(self.id)
    }
}

impl<R: Resource> Clone for Slot<R> {
    fn clone(&self) -> Self {
        Self {
            space: self.space.clone(),
            id: self.id,
            _phantom: PhantomData,
        }
    }
}
