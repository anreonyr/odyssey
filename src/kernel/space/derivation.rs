//! Derivation paths — `grant` / `transfer` / `restrict`.
//!
//! The body lives here; the lock acquisition and slot insertion
//! live in `super::install_derived_inner`, which holds the
//! canonical lock order and is `pub(super)` to enforce layering.
//!
//! Phase 5 fixes embedded here:
//!
//! - **D2** (`install_derived` parent-live check): the inner
//!   install helper refuses when `parent` is no longer in
//!   `parents`. Closes the Interleaving 2 race in
//!   `revoke_tree` × `grant`.
//! - **R4/R5/R7**: `grant`/`transfer`/`restrict` share the same
//!   `derive_with` body. The previous 3×~30 LOC duplication is gone.

use std::sync::Arc;

use crate::kernel::cap::Capability;
use crate::kernel::error::CapabilityError;
use crate::kernel::ids::SlotId;
use crate::kernel::resource::Resource;
use crate::kernel::rights::CapabilityRights;

use super::{CapabilitySpace, DeriveKind, GraphEvent};

/// Selector for revoke semantics. Phase 5 R6 fix: replaces the
/// Phase 4 `revoke` / private `revoke_with_sweep` split.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RevokeMode {
    /// Clear this slot only. Do not touch descendants. Used by
    /// `transfer` (the source slot, which has just been replaced
    /// by a freshly-installed derived slot).
    Single,
    /// Clear this slot and every descendant. Used by `revoke_tree`.
    Tree,
}

/// **Grant** — derive a new slot with the given rights; source
/// unchanged. seL4: CNode.Mint.
pub fn grant<R: Resource>(
    space: &CapabilitySpace,
    from: SlotId,
    rights: CapabilityRights,
    new_name: String,
) -> Result<SlotId, CapabilityError> {
    derive_with::<R>(space, from, rights, new_name, DeriveKind::Grant)
}

/// **Transfer** — move the capability to a fresh slot. Source cleared.
/// seL4: CNode.Move.
pub fn transfer<R: Resource>(
    space: &CapabilitySpace,
    from: SlotId,
    rights: CapabilityRights,
) -> Result<SlotId, CapabilityError> {
    let source_name = space.name_for_slot(from).unwrap_or_default();
    let new_slot = derive_with::<R>(space, from, rights, source_name, DeriveKind::Transfer)?;
    // The transfer target inherits the source's registered name;
    // revoke the source with `Single` mode so we don't sweep the
    // just-installed child.
    super::revocation::revoke(space, from, super::RevokeMode::Single);
    Ok(new_slot)
}

/// **Restrict** — derive a strictly less-powerful view of the source.
/// seL4: CNode.Mutate.
pub fn restrict<R: Resource>(
    space: &CapabilitySpace,
    from: SlotId,
    rights: CapabilityRights,
    new_name: String,
) -> Result<SlotId, CapabilityError> {
    derive_with::<R>(space, from, rights, new_name, DeriveKind::Restrict)
}

/// Shared body for grant / transfer / restrict. Phase 5 R4/R5:
/// replaces the Phase 4 ~110 LOC of duplicated bodies.
fn derive_with<R: Resource>(
    space: &CapabilitySpace,
    from: SlotId,
    rights: CapabilityRights,
    new_name: String,
    kind: DeriveKind,
) -> Result<SlotId, CapabilityError> {
    let source: Arc<Capability<R>> = space
        .lookup_typed::<R>(from)
        .ok_or(CapabilityError::SlotEmpty(from))?;
    let held = source.rights();
    if !held.contains(&rights) {
        return Err(CapabilityError::AttenuationViolation {
            from,
            requested: rights.operations,
            held: held.operations,
        });
    }
    let new_id = space.next_derived_id();
    let derived = source.derive(rights, new_id);
    let new_slot = space.install_derived(from, derived, new_name)?;
    space.publish_event(GraphEvent::Derived {
        parent: from,
        child: new_slot,
        kind,
    });
    Ok(new_slot)
}
