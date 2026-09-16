//! Personality / lifecycle / ruin — reverse-mint revocation.
//!
//! Phase 8: moved from `src/runtime/teardown.rs`. Tears down
//! runtime plugins in **reverse** mint order — the symmetric
//! counterpart to `mint_all` in `mint.rs`. Each plugin's minted
//! slot ids are passed to `cspace.revoke_tree`, which removes the
//! slot and any descendants (derived caps from `restrict`/`grant`).
//!
//! Order matters: consumers die **before** providers, so any
//! in-flight work the consumer was doing on the provider's cap
//! sees `Slot::capability() → None` rather than racing the
//! provider's teardown.
//!
//! ## Naming note (preserved across the move)
//!
//! The function is named `ruin_runtime_plugins`. Phase 4
//! introduced it as `shutdown_runtime_plugins`; commit
//! `ee680e3` renamed it to `ruin_runtime_plugins`. The name
//! is the deliberate English-verb dual of `mint_runtime_plugins`:
//! `mint` is the verb for capability creation, `ruin` is the
//! verb for wholesale capability destruction. **Do not normalise
//! this name without an explicit owner decision** — see
//! `CHANGELOG.md` Phase 7 entry. The `ruin` name travels with the
//! function across the Phase 8 migration.

use std::collections::HashMap;

use crate::capability::enforce::space::CapabilitySpace;
use crate::core::identity::ids::{PluginId, SlotId};
use crate::personality::composition::resolve::ResolvedPlan;
use crate::personality::lifecycle::lifecycle_event::{LifecycleEvent, LifecycleEventBus};

pub async fn ruin_runtime_plugins(
    cspace: &CapabilitySpace,
    lifecycle: &LifecycleEventBus,
    plan: &ResolvedPlan,
    minted: &HashMap<PluginId, Vec<SlotId>>,
) {
    println!("\n[shutdown] tearing down runtime plugins (reverse mint order):");
    for plugin_id in plan.mint_order.iter().rev() {
        let Some(slot_ids) = minted.get(plugin_id) else {
            continue;
        };
        // Phase 8 split: lifecycle events go to the
        // personality's own bus, not the kernel's capability bus.
        // Order:
        //   LifecycleEvent::PluginDeactivated { plugin }
        //   (per slot:)
        //     CapabilityEvent::Revoked { slot, capability }
        //     CapabilityEvent::RevokeTree { root, total }
        // — one PluginDeactivated per plugin, then per-slot
        // pairs of Revoked + RevokeTree.
        let _ = lifecycle.publish(LifecycleEvent::PluginDeactivated {
            plugin: plugin_id.clone(),
        });
        let mut total_revoked = 0usize;
        for slot_id in slot_ids {
            let n = cspace.revoke_tree(*slot_id);
            total_revoked += n;
        }
        if total_revoked > 0 || !slot_ids.is_empty() {
            println!(
                "  ✓ {}@{}  revoked {} slot(s)",
                plugin_id.name, plugin_id.version, total_revoked
            );
        }
    }
    let remaining = cspace.len();
    println!("[shutdown] cspace remaining slots: {remaining}");
}
