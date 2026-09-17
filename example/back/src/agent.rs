//! Agent builtin — a read-only view over a plugin's own reachable
//! capabilities.
//!
//! The agent is the first builtin that consumes its row of
//! `ResolvedPlan::bindings`. It holds the `(handle, provider,
//! capability)` triples the resolver produced for its `requires`,
//! and answers two questions about them: which handles exist, and
//! what is behind one.
//!
//! ## Why the reachable set is the resolver's table
//!
//! Every capability name the agent can act on comes out of a
//! `ResolvedBinding`. There is no path that turns an arbitrary
//! string into a lookup: `reach` searches the bindings, and only a
//! binding's own `capability` is handed to
//! `CapabilitySpace::lookup_by_name`. Reachability is therefore a
//! property of the table the agent was minted with, not a
//! convention its code follows. A handle that never appeared in a
//! `requires` is `AgentError::Unknown` no matter what the cspace
//! holds.
//!
//! ## Why nothing is snapshotted
//!
//! `CapabilityMeta` carries name, namespace, contract, kind and
//! budget — but not operation rights. Rights live on
//! `Capability<R>` and are reachable only through
//! `AnyCapability::operations()`. So the agent stores *declarations*
//! and resolves live on every call: a capability revoked after mint
//! shows up as `live: false` with the provider fields absent,
//! instead of a stale record claiming it still exists.
//!
//! ## Scope
//!
//! Read-only by decision: the agent observes capabilities and never
//! invokes one. `describe` reports a capability's `kind` and
//! `operations` without exercising either. Erased *invocation*
//! (`AnyCapability::invoke_dyn`) skips the `OperationRights` check
//! that the typed `invoke_op` performs — the HTTP bridge has the
//! same gap — so an agent that dispatched would inherit it. Closing
//! that gap is a separate line of work; this builtin stays on the
//! observing side of it.

use std::sync::Arc;

use serde_json::{Value, json};

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::capability::handle::cap::AnyCapability;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::core::rights::rights::Rights;
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};

/// Names one capability row of the agent's reachable table.
pub const CONTRACT_LIST: &str = "agent_list";
/// Names the other. Two contracts, two capabilities, one use each.
pub const CONTRACT_DESCRIBE: &str = "agent_describe";

/// Every way the agent refuses a request. The three variants are
/// three different faults: a malformed request, a handle outside
/// the reachable table, and a handle inside it with no capability
/// behind it.
#[derive(Debug, PartialEq, Eq)]
pub enum AgentError {
    /// The input was not the shape the operation documents.
    Input(String),
    /// The handle is not in the reachable table.
    Unknown(String),
    /// The handle is in the table, but its row carries no
    /// capability name. A manifest fault, not a caller fault.
    Unbound(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(message) => write!(f, "agent: {message}"),
            Self::Unknown(handle) => write!(f, "agent: unknown handle `{handle}`"),
            Self::Unbound(handle) => write!(f, "agent: handle `{handle}` has no bound capability"),
        }
    }
}

/// One row of the reachable table, resolved against the cspace.
///
/// `live` is `None` when the binding has no capability behind it
/// any more — the provider was revoked, or its slot was never
/// installed. The field is an `Option` rather than a flag beside a
/// nullable capability so that "not live, yet here is the
/// capability" has no representation.
pub struct Reach {
    pub binding: ResolvedBinding,
    pub live: Option<Arc<dyn AnyCapability>>,
}

/// The agent's whole state: the space to resolve against, and the
/// bindings it was minted with.
pub struct AgentCore {
    cspace: CapabilitySpace,
    bindings: Vec<ResolvedBinding>,
}

impl AgentCore {
    pub fn new(cspace: CapabilitySpace, bindings: Vec<ResolvedBinding>) -> Self {
        Self { cspace, bindings }
    }

    /// Resolve one handle against the reachable table first, the
    /// cspace second. Known-but-gone is a `Reach` with `live: None`.
    pub fn reach(&self, handle: &str) -> Result<Reach, AgentError> {
        let binding = self
            .bindings
            .iter()
            .find(|b| b.handle == handle)
            .ok_or_else(|| AgentError::Unknown(handle.to_string()))?
            .clone();
        if binding.capability.is_empty() {
            return Err(AgentError::Unbound(handle.to_string()));
        }
        let live = self.cspace.lookup_by_name(&binding.capability);
        Ok(Reach { binding, live })
    }

    /// Every reachable handle paired with whether it is live.
    ///
    /// The liveness of every row is resolved before any of them is
    /// returned, so an `Unbound` row fails the whole read instead
    /// of yielding a list that quietly omits one entry.
    pub fn handles(&self) -> Result<Vec<(String, bool)>, AgentError> {
        let mut resolved = Vec::with_capacity(self.bindings.len());
        for binding in &self.bindings {
            if binding.capability.is_empty() {
                return Err(AgentError::Unbound(binding.handle.clone()));
            }
            let live = self.cspace.lookup_by_name(&binding.capability).is_some();
            resolved.push((binding.handle.clone(), live));
        }
        Ok(resolved)
    }

    /// The `agent_list` payload.
    pub fn list(&self) -> Result<Value, AgentError> {
        let entries: Vec<Value> = self
            .handles()?
            .into_iter()
            .map(|(handle, live)| json!({ "handle": handle, "live": live }))
            .collect();
        Ok(json!({ "handles": entries }))
    }

    /// The `agent_describe` payload. Capability-level fields are
    /// present only while the capability is live — reporting the
    /// kind or rights of something that no longer exists would be
    /// invention.
    pub fn describe(&self, handle: &str) -> Result<Value, AgentError> {
        let Reach { binding, live } = self.reach(handle)?;
        let Some(cap) = live else {
            return Ok(json!({
                "handle": binding.handle,
                "live": false,
                "capability": binding.capability,
                "contract": binding.contract,
            }));
        };
        let meta = cap.meta();
        Ok(json!({
            "handle": binding.handle,
            "live": true,
            "capability": binding.capability,
            "contract": binding.contract,
            "name": meta.name,
            "namespace": meta.namespace,
            "plugin": meta.plugin.name.as_str(),
            "kind": kind_name(meta.kind),
            "streaming": meta.kind == CapKind::Stream,
            "timeout_ms": meta.timeout_ms,
            "calls_per_minute": meta.quota.calls_per_minute,
            "operations": operation_names(cap.operations()),
        }))
    }
}

fn kind_name(kind: CapKind) -> &'static str {
    match kind {
        CapKind::Sync => "sync",
        CapKind::Stream => "stream",
    }
}

fn operation_names(rights: Rights) -> Vec<&'static str> {
    [
        (rights.contains(Rights::INVOKE), "INVOKE"),
        (rights.contains(Rights::ASSIGN), "ASSIGN"),
        (rights.contains(Rights::REVOKE), "REVOKE"),
    ]
    .into_iter()
    .filter_map(|(held, name)| held.then_some(name))
    .collect()
}

/// Reads the only field `agent_describe` accepts.
///
/// The shape is closed on purpose. Before this was strict, a body of
/// `{"handle": "echo", "op": "invoke"}` answered with echo's
/// description and ignored `op` — a caller who believed the agent
/// dispatches would have read that as "the call went through". The
/// agent observes and never invokes, so an unknown field is refused
/// rather than dropped.
fn describe_input(input: &Value) -> Result<&str, AgentError> {
    let object = input
        .as_object()
        .ok_or_else(|| AgentError::Input("expected a JSON object".to_string()))?;
    if let Some(unknown) = object.keys().find(|k| k.as_str() != "handle") {
        return Err(AgentError::Input(format!(
            "unknown field `{unknown}`; the only accepted field is `handle`"
        )));
    }
    object
        .get("handle")
        .and_then(Value::as_str)
        .ok_or_else(|| AgentError::Input("expected a `handle` string".to_string()))
}

/// `agent_list` — the reachable handles and their liveness.
pub struct AgentListResource {
    core: AgentCore,
}

impl AgentListResource {
    pub fn new(cspace: CapabilitySpace, bindings: Vec<ResolvedBinding>) -> Self {
        Self {
            core: AgentCore::new(cspace, bindings),
        }
    }
}

impl Resource for AgentListResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        if !input.is_object() {
            return Err(AgentError::Input("expected a JSON object".to_string()).to_string());
        }
        self.core.list().map_err(|e| e.to_string())
    }
}

/// `agent_describe` — what sits behind one reachable handle.
pub struct AgentDescribeResource {
    core: AgentCore,
}

impl AgentDescribeResource {
    pub fn new(cspace: CapabilitySpace, bindings: Vec<ResolvedBinding>) -> Self {
        Self {
            core: AgentCore::new(cspace, bindings),
        }
    }
}

impl Resource for AgentDescribeResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let handle = describe_input(&input).map_err(|e| e.to_string())?;
        self.core.describe(handle).map_err(|e| e.to_string())
    }
}

/// The four capabilities the agent can reach, as `(handle,
/// contract)` pairs for its `requires`.
pub const REACHES: [(&str, &str); 4] = [
    ("echo", "echo"),
    ("reverse", "reverse"),
    ("database", "database"),
    ("streaming_echo", "streaming_echo"),
];

/// `agent_list` as a system plugin.
pub struct AgentListBuiltin;

impl BuiltinManifest for AgentListBuiltin {
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("agent_list")
            .expose(CONTRACT_LIST, CONTRACT_LIST)
            .requires("echo", "echo")
            .requires("reverse", "reverse")
            .requires("database", "database")
            .requires("streaming_echo", "streaming_echo")
            .timeout_ms(5000)
            .build()
    }
}

impl AgentListBuiltin {
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            AgentListBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                AgentListBuiltin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }

    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        bindings: &[ResolvedBinding],
    ) -> SlotId {
        use odyssey::core::rights::rights::{CapabilityRights, Rights};

        let pc = factory.plugin_cspace(plugin);
        let local_slot = pc.mint(
            kind,
            decl,
            budget.clone(),
            Arc::new(AgentListResource::new(
                factory.space().clone(),
                bindings.to_vec(),
            )),
        );
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<AgentListResource>(local_slot, factory.space(), rights, decl.name.clone())
            .expect("grant from plugin cspace to global should succeed")
    }
}

/// `agent_describe` as a system plugin.
pub struct AgentDescribeBuiltin;

impl BuiltinManifest for AgentDescribeBuiltin {
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("agent_describe")
            .expose(CONTRACT_DESCRIBE, CONTRACT_DESCRIBE)
            .requires("echo", "echo")
            .requires("reverse", "reverse")
            .requires("database", "database")
            .requires("streaming_echo", "streaming_echo")
            .timeout_ms(5000)
            .build()
    }
}

impl AgentDescribeBuiltin {
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            AgentDescribeBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                AgentDescribeBuiltin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }

    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        bindings: &[ResolvedBinding],
    ) -> SlotId {
        use odyssey::core::rights::rights::{CapabilityRights, Rights};

        let pc = factory.plugin_cspace(plugin);
        let local_slot = pc.mint(
            kind,
            decl,
            budget.clone(),
            Arc::new(AgentDescribeResource::new(
                factory.space().clone(),
                bindings.to_vec(),
            )),
        );
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<AgentDescribeResource>(
                local_slot,
                factory.space(),
                rights,
                decl.name.clone(),
            )
            .expect("grant from plugin cspace to global should succeed")
    }
}
