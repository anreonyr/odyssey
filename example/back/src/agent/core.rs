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
}

impl std::fmt::Display for AgentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(message) => write!(f, "agent: {message}"),
            Self::Unknown(handle) => write!(f, "agent: unknown handle `{handle}`"),
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
        // Try the plugin's own binding row first so a handle
        // such as "echo" resolves even though the observer
        // plugin (`agent_describe`) doesn't itself bind to it.
        // Fall back to a cspace lookup by capability name so
        // `agent_list` can return any cap's metadata through
        // the same `describe` entry point.
        let (resolved_handle, capability_name, provider) =
            if let Some(row) = self.rows.iter().find(|r| r.handle == handle) {
                (
                    row.handle.clone(),
                    row.capability.clone(),
                    row.provider.clone(),
                )
            } else if let Some(cap) = self.cspace.lookup_by_name(handle) {
                let meta = cap.meta();
                (
                    handle.to_string(),
                    meta.name.clone(),
                    meta.plugin.clone(),
                )
            } else {
                return Err(AgentError::Unknown(handle.to_string()));
            };

        let cap = self.cspace.lookup_by_name(&capability_name);
        match cap {
            None => Ok(json!({
                "handle": resolved_handle,
                "provider": { "name": provider.name, "version": provider.version },
                "live": false,
                "capability": capability_name,
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
                    "handle": resolved_handle,
                    "provider": { "name": provider.name, "version": provider.version },
                    "live": true,
                    "capability": capability_name,
                    "kind": kind,
                    "operations": ops,
                }))
            }
        }
    }

    pub fn list(&self) -> Result<Value, AgentError> {
        // The plugin that owns this `AgentCore` may have an
        // empty binding row (the observer plugins — `agent_list`
        // and `agent_describe` — declare no `requires`), so
        // walking `self.rows` alone produces an empty list and
        // the UI shows every cap as "not in binding row". The
        // observer's job is to answer "what is in the cspace
        // and what is behind it", not "what does this one
        // plugin bind to".
        //
        // Walk the global cspace and return one entry per
        // capability. Each entry names its plugin so the UI can
        // group caps by source and surface provider metadata.
        // `handle` mirrors `capability` for caps the consumer
        // doesn't bind to (the observer's empty row case).
        let binding_handles: std::collections::HashMap<&str, &str> = self
            .rows
            .iter()
            .filter(|r| !r.capability.is_empty())
            .map(|r| (r.capability.as_str(), r.handle.as_str()))
            .collect();
        let mut handles: Vec<Value> = Vec::new();
        for meta in self.cspace.enumerate() {
            let handle = binding_handles
                .get(meta.name.as_str())
                .copied()
                .unwrap_or(meta.name.as_str());
            handles.push(json!({
                "handle": handle,
                "capability": meta.name,
                "live": true,
                "provider": {
                    "name": meta.plugin.name,
                    "version": meta.plugin.version,
                },
            }));
        }
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
