//! Observer half of the agent — read-only views over a
//! plugin's reachable capabilities. Lives in the same
//! `agent/` module as the runtime half (`runtime.rs` /
//! `builtin.rs`) but keeps its own `AgentError` and
//! `AgentCore` so the two halves don't share identifier
//! names.
//!
//! The first builtin that consumes its row of
//! `ResolvedPlan::bindings`. It holds the
//! `(handle, provider, capability)` triples the resolver
//! produced for its `requires`, and answers two questions:
//! which handles exist, and what is behind one.
//!
//! ## Why the reachable set is the resolver's table
//!
//! Every capability name the agent can act on comes out of
//! a `ResolvedBinding`. There is no path that turns an
//! arbitrary string into a lookup: `reach` searches the
//! bindings, and only a binding's own `capability` is
//! handed to `CapabilitySpace::lookup_by_name`. Reachability
//! is therefore a property of the table the agent was
//! minted with, not a convention its code follows. A handle
//! that never appeared in a `requires` is `AgentError::Unknown`
//! no matter what the cspace holds.
//!
//! ## Why nothing is snapshotted
//!
//! `CapabilityMeta` carries name, namespace, contract, kind
//! and budget — but not operation rights. Rights live on
//! `Capability<R>` and are reachable only through
//! `AnyCapability::operations()`. So the agent stores
//! *declarations* and resolves live on every call.
//!
//! ## Scope
//!
//! Read-only by decision: the agent observes capabilities
//! and never invokes one. `describe` reports a capability's
//! `kind` and `operations` without exercising either.

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
use odyssey::personality::lifecycle::mint::{CapabilityFactory, MintError, TypedBindings};
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};

pub const CONTRACT_LIST: &str = "agent_list";
pub const CONTRACT_DESCRIBE: &str = "agent_describe";

#[derive(Debug, PartialEq, Eq)]
pub enum AgentError {
    Input(String),
    Unknown(String),
    Unbound(String),
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(message) => write!(f, "agent: {message}"),
            Self::Unknown(handle) => write!(f, "agent: unknown handle `{handle}`"),
            Self::Unbound(handle) => {
                write!(f, "agent: handle `{handle}` has no bound capability")
            }
        }
    }
}

pub struct Reach {
    pub binding: ResolvedBinding,
    pub live: Option<Arc<dyn AnyCapability>>,
}

pub struct AgentCore {
    cspace: CapabilitySpace,
    rows: Vec<ResolvedBinding>,
}

impl AgentCore {
    pub fn new(cspace: CapabilitySpace, rows: Vec<ResolvedBinding>) -> Self {
        Self { cspace, rows }
    }

    pub fn reach(&self, handle: &str) -> Result<Reach, AgentError> {
        let row = self
            .rows
            .iter()
            .find(|r| r.handle == handle)
            .ok_or_else(|| AgentError::Unknown(handle.to_string()))?;
        let live = if row.capability.is_empty() {
            None
        } else {
            self.cspace.lookup_by_name(&row.capability)
        };
        Ok(Reach {
            binding: row.clone(),
            live,
        })
    }

    pub fn describe(&self, handle: &str) -> Result<Value, AgentError> {
        let reach = self.reach(handle)?;
        match reach.live {
            None => Ok(json!({
                "handle": reach.binding.handle,
                "provider": {
                    "name": reach.binding.provider.name,
                    "version": reach.binding.provider.version,
                },
                "live": false,
                "capability": reach.binding.capability,
                "unbound": true,
            })),
            Some(cap) => {
                let meta = cap.meta();
                let kind = match meta.kind {
                    CapKind::Sync => "sync",
                    CapKind::Stream => "stream",
                };
                let ops: Vec<&str> = [
                    (cap.operations().contains(Rights::INVOKE), "INVOKE"),
                    (cap.operations().contains(Rights::ASSIGN), "ASSIGN"),
                    (cap.operations().contains(Rights::REVOKE), "REVOKE"),
                ]
                .into_iter()
                .filter_map(|(held, name)| held.then_some(name))
                .collect();
                Ok(json!({
                    "handle": reach.binding.handle,
                    "provider": {
                        "name": reach.binding.provider.name,
                        "version": reach.binding.provider.version,
                    },
                    "live": true,
                    "capability": reach.binding.capability,
                    "kind": kind,
                    "operations": ops,
                }))
            }
        }
    }

    pub fn list(&self) -> Result<Value, AgentError> {
        let handles: Vec<Value> = self
            .rows
            .iter()
            .map(|row| {
                let live = if row.capability.is_empty() {
                    None
                } else {
                    self.cspace.lookup_by_name(&row.capability)
                };
                match live {
                    None => Err(AgentError::Unbound(row.handle.clone())),
                    Some(_) => Ok(json!({
                        "handle": row.handle,
                        "provider": {
                            "name": row.provider.name,
                            "version": row.provider.version,
                        },
                        "capability": row.capability,
                    })),
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(json!({ "handles": handles }))
    }
}

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

fn list_input(input: &Value) -> Result<(), AgentError> {
    let object = input
        .as_object()
        .ok_or_else(|| AgentError::Input("expected a JSON object".to_string()))?;
    if !object.is_empty() {
        let unknown = object.keys().next().unwrap();
        return Err(AgentError::Input(format!(
            "unknown field `{unknown}`; the only accepted input is an empty object"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `agent_list` resource
// ---------------------------------------------------------------------------

pub struct AgentListResource {
    core: AgentCore,
}

impl AgentListResource {
    pub fn new(cspace: CapabilitySpace, rows: Vec<ResolvedBinding>) -> Self {
        Self {
            core: AgentCore::new(cspace, rows),
        }
    }
}

impl Resource for AgentListResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        list_input(&input).map_err(|e| e.to_string())?;
        self.core.list().map_err(|e| e.to_string())
    }
}

pub struct AgentListBuiltin;

impl BuiltinManifest for AgentListBuiltin {
    type Resource = AgentListResource;
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("agent_list")
            .expose(CONTRACT_LIST, CONTRACT_LIST)
            .timeout_ms(5000)
            .build()
    }
}

impl AgentListBuiltin {
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            AgentListBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                AgentListBuiltin.mint(
                    factory,
                    plugin,
                    decl,
                    kind,
                    budget,
                    bindings,
                    typed_bindings,
                )
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
        _typed_bindings: &TypedBindings,
    ) -> Result<SlotId, MintError> {
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
            .map_err(|e| MintError::GrantFailed {
                plugin: plugin.name.clone(),
                cap: decl.name.clone(),
                source: e,
            })
    }
}

// ---------------------------------------------------------------------------
// `agent_describe` resource — observes the agent's reachable set
// ---------------------------------------------------------------------------

pub struct AgentDescribeResource {
    core: AgentCore,
}

impl AgentDescribeResource {
    pub fn new(cspace: CapabilitySpace, rows: Vec<ResolvedBinding>) -> Self {
        Self {
            core: AgentCore::new(cspace, rows),
        }
    }
}

impl Resource for AgentDescribeResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let handle = describe_input(&input).map_err(|e| e.to_string())?;
        self.core.describe(handle).map_err(|e| e.to_string())
    }
}

pub struct AgentDescribeBuiltin;

impl BuiltinManifest for AgentDescribeBuiltin {
    type Resource = AgentDescribeResource;
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("agent_describe")
            .expose(CONTRACT_DESCRIBE, CONTRACT_DESCRIBE)
            .requires("echo", "echo")
            .requires("database", "database")
            .timeout_ms(5000)
            .build()
    }
}

impl AgentDescribeBuiltin {
    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            AgentDescribeBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                AgentDescribeBuiltin.mint(
                    factory,
                    plugin,
                    decl,
                    kind,
                    budget,
                    bindings,
                    typed_bindings,
                )
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
        _typed_bindings: &TypedBindings,
    ) -> Result<SlotId, MintError> {
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
            .map_err(|e| MintError::GrantFailed {
                plugin: plugin.name.clone(),
                cap: decl.name.clone(),
                source: e,
            })
    }
}
