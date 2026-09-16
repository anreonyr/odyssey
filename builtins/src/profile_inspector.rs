//! Profile inspector builtin — answers "what is this cap's runtime
//! shape?" by reading `cspace.lookup_by_name(subject).meta()`.
//!
//! Where `tool_descriptor` is about the agent-facing contract
//! (input/output JSON Schema for tool calls), `profile_inspector`
//! is about the kernel-facing shape (id, namespace, contract, kind,
//! budget, quota, operations). The two are complementary readers
//! over the same `CapabilityMeta` struct.
//!
//! ## Why this is its own plugin
//!
//! It uses no agent-specific logic — anyone with a cap name can
//! ask "what is this?". The existing `agent_describe` cap already
//! does something similar for the agent's reachable set, but it's
//! gated by the agent's binding table: a cap outside the table is
//! invisible. `profile_inspector` is open: any cap in the cspace
//! can be inspected.
//!
//! ## Input / output
//!
//! ```text
//! in:  { "subject": "<capability_name>" }
//! out: { "subject": "...", "meta": { ...CapabilityMeta fields... },
//!        "operations": ["READ", "EXECUTE", ...] }
//! ```
//!
//! Errors:
//! - `Input` — input shape wrong.
//! - `SubjectNotFound` — no cap by that name.

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::core::rights::rights::OperationRights;
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};

pub const CONTRACT: &str = "profile_inspector";
pub const NAME: &str = "profile_inspect";

#[derive(Debug, PartialEq, Eq)]
pub enum ProfileInspectorError {
    Input(String),
    SubjectNotFound(String),
}

impl std::fmt::Display for ProfileInspectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(message) => write!(f, "profile_inspector: {message}"),
            Self::SubjectNotFound(name) => {
                write!(f, "profile_inspector: subject `{name}` not found in cspace")
            }
        }
    }
}

pub struct ProfileInspectorResource {
    cspace: CapabilitySpace,
}

impl ProfileInspectorResource {
    pub fn new(cspace: CapabilitySpace) -> Self {
        Self { cspace }
    }

    fn inspect(&self, subject: &str) -> Result<Value, ProfileInspectorError> {
        let cap = self
            .cspace
            .lookup_by_name(subject)
            .ok_or_else(|| ProfileInspectorError::SubjectNotFound(subject.to_string()))?;

        let meta = cap.meta();
        let kind_name = match meta.kind {
            CapKind::Sync => "sync",
            CapKind::Stream => "stream",
        };

        Ok(json!({
            "subject": subject,
            "meta": {
                "id": meta.id.to_string(),
                "name": meta.name,
                "namespace": meta.namespace,
                "contract_name": meta.contract_name,
                "plugin": { "name": meta.plugin.name, "version": meta.plugin.version },
                "in_type": meta.in_type,
                "out_type": meta.out_type,
                "kind": kind_name,
                "timeout_ms": meta.timeout_ms,
                "quota": {
                    "calls_per_minute": meta.quota.calls_per_minute,
                },
                "has_tool_schema": meta.tool_schema.is_some(),
            },
            "operations": operation_names(cap.operations()),
        }))
    }
}

fn operation_names(rights: OperationRights) -> Vec<&'static str> {
    [
        (rights.contains(OperationRights::READ), "READ"),
        (rights.contains(OperationRights::WRITE), "WRITE"),
        (rights.contains(OperationRights::EXECUTE), "EXECUTE"),
        (rights.contains(OperationRights::ADMIN), "ADMIN"),
    ]
    .into_iter()
    .filter_map(|(held, name)| held.then_some(name))
    .collect()
}

fn inspect_input(input: &Value) -> Result<&str, ProfileInspectorError> {
    let object = input
        .as_object()
        .ok_or_else(|| ProfileInspectorError::Input("expected a JSON object".to_string()))?;
    if let Some(unknown) = object.keys().find(|k| k.as_str() != "subject") {
        return Err(ProfileInspectorError::Input(format!(
            "unknown field `{unknown}`; the only accepted field is `subject`"
        )));
    }
    object
        .get("subject")
        .and_then(Value::as_str)
        .ok_or_else(|| ProfileInspectorError::Input("expected a `subject` string".to_string()))
}

impl Resource for ProfileInspectorResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let subject = inspect_input(&input).map_err(|e| e.to_string())?;
        self.inspect(subject).map_err(|e| e.to_string())
    }
}

pub struct ProfileInspectorBuiltin;

impl BuiltinManifest for ProfileInspectorBuiltin {
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("profile_inspector")
            .expose(NAME, CONTRACT)
            .host("dispatcher")
            .timeout_ms(5000)
            .build()
    }
}

impl ProfileInspectorBuiltin {
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
    ) -> SlotId {
        let resource = Arc::new(ProfileInspectorResource::new(factory.space().clone()));
        factory.mint(kind, decl, plugin, budget, resource)
    }

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            ProfileInspectorBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings| {
                ProfileInspectorBuiltin.mint(factory, plugin, decl, kind, budget, bindings)
            },
            default_ruin,
        )
    }
}
