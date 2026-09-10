//! `mint_simple` — the typed mint path for plugins that
//! don't need to consult the resolver at mint time.
//!
//! Every `[[exposes]]` block produces one typed
//! `Capability<R>`, installed under `slot_key`. The mint
//! function returns the freshly minted slot ids so the
//! caller can revoke them at teardown.

use std::sync::Arc;

use crate::host::factory::CapabilityFactory;
use crate::host::manifest::{CapabilityDecl, PluginManifest};
use crate::kernel::ids::{PluginId, SlotId};
use crate::kernel::quota::CapabilityBudget;
use crate::kernel::resource::Resource;
use crate::kernel::{CapKind, Slot};

/// Mint a "simple" plugin: every `[[exposes]]` block produces
/// one typed `Capability<R>`, installed under `slot_key`.
///
/// `handler_for` is a closure that builds the typed resource
/// for a `(decl, plugin)` pair; simple plugins return a
/// stateless or pre-built handler, but the closure shape
/// keeps the call site uniform across all simple plugins.
///
/// Returns the freshly minted slot ids so the caller can
/// revoke them at teardown.
pub async fn mint_simple<R, F>(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    m: &PluginManifest,
    kind: CapKind,
    slot_key: &'static str,
    handler_for: F,
) -> Result<Vec<SlotId>, Box<dyn std::error::Error>>
where
    R: Resource + 'static,
    F: Fn(&CapabilityDecl, &PluginId) -> Arc<R>,
{
    let mut slots = Vec::with_capacity(m.exposes.len());
    for cap in &m.exposes {
        let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
        let handler = handler_for(cap, &m.plugin);
        let slot_id = factory.mint::<R>(kind, cap, &m.plugin, budget, handler);
        let slot = Slot::<R>::new(factory.space().clone(), slot_id);
        ctx.provide(slot_key, slot)
            .await
            .map_err(|e| format!("provide {slot_key}: {e}"))?;
        println!(
            "    {:<14} → slot={slot_id}  contract={}",
            cap.name, cap.contract_name
        );
        slots.push(slot_id);
    }
    Ok(slots)
}
