//! `mint_echo_chain` — the mint path for echo-chain. The only
//! runtime plugin that needs a cross-plugin capability at
//! mint time; it closes over the typed `Capability<EchoResource>`
//! from the resolver's binding table.

use std::sync::Arc;

use crate::host::factory::CapabilityFactory;
use crate::host::manifest::PluginManifest;
use crate::host::resolver::ResolvedPlan;
use crate::kernel::cap::Capability;
use crate::kernel::ids::SlotId;
use crate::kernel::quota::CapabilityBudget;
use crate::kernel::{CapKind, CapabilitySpace, Slot};
use crate::plugins::echo::basic::EchoResource;

/// echo-chain is the only runtime plugin that needs a
/// cross-plugin capability at mint time: it closes over the
/// typed `Capability<EchoResource>`. The cap is delivered via
/// the **resolved binding table** — echo-chain's manifest
/// declares `[[requires]] name="echo" contract="echo"` and the
/// resolver walks it to find which provider fulfils the
/// contract. `plan.bindings[m.plugin]` carries that mapping;
/// we read the binding's `capability` field and look up the
/// cap by that name in cspace. The provider was minted
/// earlier in `plan.mint_order` (topologically), so the cap
/// is already in cspace by the time we reach this branch.
pub async fn mint_echo_chain(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    m: &PluginManifest,
) -> Result<Vec<SlotId>, Box<dyn std::error::Error>> {
    // 1) Find the binding for echo-chain's `echo` handle.
    let bindings = plan
        .bindings
        .get(&m.plugin)
        .ok_or_else(|| format!("echo-chain: no bindings for {} in plan", m.plugin.name))?;
    let echo_binding = bindings
        .iter()
        .find(|b| b.handle == "echo")
        .ok_or_else(|| {
            format!(
                "echo-chain: no binding for handle \"echo\" (requires = {:?})",
                m.requires
            )
        })?;

    // 2) Look up the cap by the binding's `capability` field.
    //    This is the same lookup the resolver performed at
    //    plan time — by following it again here, the runtime
    //    path remains driven entirely by the plan; if the
    //    resolver changed how it picks providers (e.g.
    //    version ranges), echo-chain automatically tracks.
    let echo_cap = cspace
        .lookup_by_name(&echo_binding.capability)
        .ok_or_else(|| {
            format!(
                "echo-chain: capability \"{}\" (contract {}) not in cspace",
                echo_binding.capability, echo_binding.contract
            )
        })?;
    let typed = echo_cap
        .as_any()
        .downcast_ref::<Capability<EchoResource>>()
        .ok_or_else(|| {
            format!(
                "echo-chain: capability \"{}\" has wrong type (expected Capability<EchoResource>)",
                echo_binding.capability
            )
        })?;
    let typed_arc = Arc::new(typed.clone());

    let mut slots = Vec::with_capacity(m.exposes.len());
    for cap in &m.exposes {
        let budget = CapabilityBudget::new(m.resources.timeout_ms.unwrap_or(5000));
        let slot_id = factory.mint::<crate::plugins::echo::chain::EchoChainResource>(
            CapKind::Sync,
            cap,
            &m.plugin,
            budget,
            crate::plugins::echo::chain::handler(typed_arc.clone()),
        );
        let slot = Slot::<crate::plugins::echo::chain::EchoChainResource>::new(
            cspace.clone(),
            slot_id,
        );
        ctx.provide("slot:echo_chain", slot)
            .await
            .map_err(|e| format!("provide slot:echo_chain: {e}"))?;
        println!(
            "    {:<14} → slot={slot_id}  contract={}",
            cap.name, cap.contract_name
        );
        slots.push(slot_id);
    }
    Ok(slots)
}
