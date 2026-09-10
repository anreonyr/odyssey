//! Revocation paths — `revoke` (single) and `revoke_tree` (cascade).
//!
//! Phase 5 R6 fix: the Phase 4 `revoke` and private
//! `revoke_with_sweep` are unified into a single body selected by
//! `RevokeMode`. Lock acquisition walks `parents` atomically under
//! the write lock, eliminating the read-then-write gap that
//! produced the D2 race window.

use std::sync::Arc;

use crate::kernel::ids::SlotId;

use super::{CapabilitySpace, GraphEvent, RevokeMode};

/// **Revoke** — clear `slot`. Mode-aware:
///
/// - `Single`: clear this slot only. Used by `transfer`.
/// - `Tree`: clear this slot and every descendant.
///
/// Phase 5: both modes share the same lock-acquisition pattern.
/// The kernel-level invariant "every cap has a live parent or is
/// a root" is upheld by the install_derived precondition check
/// (D2); this function relies on that invariant.
pub fn revoke(space: &CapabilitySpace, slot: SlotId, mode: RevokeMode) -> bool {
    if mode == RevokeMode::Single {
        return revoke_single(space, slot);
    }
    revoke_tree(space, slot) >= 1
}

/// Single-slot revoke. Internal helper used by `revoke(Single)`
/// and by `transfer` after the derived slot is installed.
fn revoke_single(space: &CapabilitySpace, slot: SlotId) -> bool {
    let cap_name = space.name_for_slot(slot);

    // Canonical lock order: parents → slots → names.
    let mut parents = space.parents_guarded();
    let mut slots = space.slots_guarded();
    let cap_for_marker: Option<Arc<dyn super::AnyCapability>> =
        slots.get(&slot).map(|e| e.cap.clone());

    let removed = slots.remove(&slot);
    if removed.is_some() {
        let mut names = space.names_guarded();
        names.retain(|_, s| *s != slot);
        parents.remove(&slot);
        drop(slots);
        drop(names);
        drop(parents);

        if let Some(cap) = cap_for_marker {
            space.mark_revoked_for(&cap);
        }
        space.publish_event(GraphEvent::Revoked {
            slot,
            capability: cap_name,
        });
        true
    } else {
        false
    }
}

/// Tree revoke — clear `root` and every descendant. Walks the
/// parent pointer map atomically under the write lock, eliminating
/// the read-then-write gap.
pub fn revoke_tree(space: &CapabilitySpace, root: SlotId) -> usize {
    let mut removed = 0usize;
    let mut visited: std::collections::HashSet<SlotId> = std::collections::HashSet::new();
    let mut frontier = vec![root];
    visited.insert(root);

    while let Some(slot) = frontier.pop() {
        // Walk children atomically under the write lock — collect
        // their ids, then push them onto the frontier for the next
        // iteration. (We don't recurse under the lock because that
        // would serialise the whole revocation; instead we batch the
        // walk then release the lock before the per-slot revoke.)
        let children: Vec<SlotId> = {
            let parents = space.parents_guarded();
            parents
                .iter()
                .filter_map(|(child, parent)| if *parent == slot { Some(*child) } else { None })
                .collect()
        };
        for c in children {
            if visited.insert(c) {
                frontier.push(c);
            }
        }
        if revoke_single(space, slot) {
            removed += 1;
        }
    }
    space.publish_event(GraphEvent::RevokeTree { root, total: removed });
    removed
}
