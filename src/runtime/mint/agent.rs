//! `mint_agent` — the mint path for the Capability-Native Agent.
//!
//! Like echo-chain, the agent has no `[[requires]]` of its own
//! — it depends on the runtime to grant capabilities via the
//! binding table. At mint time, we hand it the cspace + a
//! consumer plugin id (this agent's own PluginId). Its
//! reachable set is whatever the resolver put in
//! `plan.bindings[&m.plugin]`.
//!
//! If the agent has no bindings (env grants nothing), the
//! reachable set is empty and every program step will skip —
//! the operator sees an empty world.

use crate::host::factory::CapabilityFactory;
use crate::host::manifest::PluginManifest;
use crate::host::resolver::ResolvedPlan;
use crate::kernel::ids::SlotId;
use crate::kernel::CapabilitySpace;

use super::simple;

pub async fn mint_agent(
    ctx: &cordis::Context,
    factory: &CapabilityFactory,
    cspace: &CapabilitySpace,
    plan: &ResolvedPlan,
    m: &PluginManifest,
) -> Result<Vec<SlotId>, Box<dyn std::error::Error>> {
    use crate::kernel::CapKind;
    let resource = crate::plugins::agent::handler_from_plan(
        m.plugin.name.clone(),
        plan,
        &m.plugin,
        cspace.clone(),
    );
    simple::mint_simple::<crate::plugins::agent::AgentResource, _>(
        ctx,
        factory,
        m,
        CapKind::Stream,
        "slot:agent",
        move |_, _| resource.clone(),
    )
    .await
}
