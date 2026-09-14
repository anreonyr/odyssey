//! Teardown — reverse mint order revocation.
//!
//! Phase 5: extracted from the Phase 4 `boot::lifecycle`.
//! Phase 3 P3.6 + P3.7 — Runtime Lifetime.
//!
//! Tear down runtime plugins in **reverse** mint order — the
//! symmetric counterpart to `mint_runtime_plugins`. Each
//! plugin's minted slot ids are passed to
//! `cspace.revoke_tree`, which removes the slot and any
//! descendants (derived caps from `restrict`/`grant`). After
//! this returns, every runtime slot is freed; consumer
//! binding entries pointing at revoked caps return `None`
//! from `cspace.lookup_by_name`.
//!
//! Order matters: consumers die **before** providers, so any
//! in-flight work the consumer was doing on the provider's
//! cap sees `Slot::capability() → None` rather than racing
//! the provider's teardown.
//!
//! ## Naming note
//!
//! The function is named `ruin_runtime_plugins` (Phase 4
//! introduced it as `shutdown_runtime_plugins`; commit
//! `ee680e3` renamed it to `ruin_runtime_plugins`). The name
//! is the deliberate English-verb dual of `mint_runtime_plugins`
//! in the runtime layer: `mint` is the verb for capability
//! creation, `ruin` is the verb for wholesale capability
//! destruction. The surrounding `teardown` module name and
//! `[shutdown]` log prefix are layer-local terminology for
//! the same lifecycle event; the function name carries the
//! duality. Do not normalise this name without an explicit
//! owner decision — see `CHANGELOG.md` Phase 7 entry.

use std::collections::HashMap;

use crate::host::resolver::ResolvedPlan;
use crate::kernel::ids::{PluginId, SlotId};
use crate::kernel::CapabilitySpace;

pub async fn ruin_runtime_plugins(
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    minted: &HashMap<PluginId, Vec<SlotId>>,
) {
    println!("\n[shutdown] tearing down runtime plugins (reverse mint order):");
    for plugin_id in plan.mint_order.iter().rev() {
        let Some(slot_ids) = minted.get(plugin_id) else {
            continue;
        };
        // Phase 3 P3.7 — emit PluginDeactivated before
        // revoking. The subsequent `revoke_tree` calls emit
        // Revoked + RevokeTree events from cspace. Order:
        //   PluginDeactivated { plugin }
        //   (per slot:)
        //     Revoked { slot, capability }
        //     RevokeTree { root, total }
        // — one PluginDeactivated per plugin, then per-slot
        // pairs of Revoked + RevokeTree. So a plugin with N
        // [[exposes]] blocks emits 1 + 2*N events from this
        // loop (plus any Revoked events from descendants
        // reached via revoke_tree). The per-slot granularity
        // gives audit logs a record of each individual cap
        // revocation; see ζ.16 for the single-slot case and
        // ζ.17 (multi_slot_plugin_shutdown) for the multi-slot
        // case.
        cspace.publish_event(
            crate::capability::enforce::space::GraphEvent::PluginDeactivated {
                plugin: plugin_id.clone(),
            },
        );
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
