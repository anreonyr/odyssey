//! Capability inspectors — two read-only plugins that introspect
//! the cspace from different angles:
//!
//! - `inspector` (`profile_inspect` cap): kernel-facing shape of a
//!   cap (id, namespace, contract, kind, budget, operations).
//! - `schema_inspector` (`schema_describe` cap): agent-facing
//!   contract of a tool (input/output JSON Schema).
//!
//! Both are peer plugins: anyone with a cap name can ask
//! "what is this?". They were separate files (`profile_inspector`
//! + `tool_descriptor`) and merged here because they share the
//! same pattern (read-only cspace lookup, single Resource, two
//! flavours of error) and the same module weight (~200 lines each).
//!
//! ## Input / output shapes
//!
//! `profile_inspect`:
//! ```text
//! in:  { "subject": "<capability_name>" }
//! out: { "subject": "...", "meta": { ...CapabilityMeta fields... },
//!        "operations": ["INVOKE", "ASSIGN", "REVOKE"] }
//! ```
//!
//! `schema_describe`:
//! ```text
//! in:  { "tool": "<capability_name>" }
//! out: { "name": "...", "description": "...",
//!        "input_schema": <JSON Schema>, "output_schema": <JSON Schema>,
//!        "operations": [...] }
//! ```

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::core::Resource;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::{CapabilityDecl, ManifestBuilder, PluginManifest};
use odyssey::core::rights::rights::Rights;
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::mint::{CapabilityFactory, MintError, TypedBindings};
use odyssey::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};
use serde_json::{Value, json};

// ===========================================================================
// Profile inspector (`profile_inspect` cap)
// ===========================================================================

pub const PROFILE_INSPECT_NAME: &str = "profile_inspect";
pub const PROFILE_INSPECT_CONTRACT: &str = "inspector";

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

fn profile_inspect_input(input: &Value) -> Result<&str, ProfileInspectorError> {
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
        let subject = profile_inspect_input(&input).map_err(|e| e.to_string())?;
        self.inspect(subject).map_err(|e| e.to_string())
    }
}

pub struct ProfileInspectorBuiltin;

impl BuiltinManifest for ProfileInspectorBuiltin {
    type Resource = ProfileInspectorResource;
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("inspector")
            .expose(PROFILE_INSPECT_NAME, PROFILE_INSPECT_CONTRACT)
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
        _typed_bindings: &TypedBindings,
    ) -> Result<SlotId, MintError> {
        use odyssey::core::rights::rights::{CapabilityRights, Rights};

        let pc = factory.plugin_cspace(plugin);
        let local_slot = pc.mint(
            kind,
            decl,
            budget.clone(),
            Arc::new(ProfileInspectorResource::new(factory.space().clone())),
        );
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<ProfileInspectorResource>(
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

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            ProfileInspectorBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                ProfileInspectorBuiltin.mint(
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
}

// ===========================================================================
// Schema inspector (`schema_describe` cap)
// ===========================================================================

pub const SCHEMA_DESCRIBE_NAME: &str = "schema_describe";
pub const SCHEMA_DESCRIBE_CONTRACT: &str = "schema_inspector";

#[derive(Debug, PartialEq, Eq)]
pub enum SchemaInspectorError {
    Input(String),
    ToolNotFound(String),
    SchemaMissing(String),
}

impl std::fmt::Display for SchemaInspectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(message) => write!(f, "schema_inspector: {message}"),
            Self::ToolNotFound(name) => {
                write!(f, "schema_inspector: tool `{name}` not found in cspace")
            }
            Self::SchemaMissing(name) => write!(
                f,
                "schema_inspector: tool `{name}` exists but has no `tool_schema` declared"
            ),
        }
    }
}

pub struct SchemaInspectorResource {
    cspace: CapabilitySpace,
}

impl SchemaInspectorResource {
    pub fn new(cspace: CapabilitySpace) -> Self {
        Self { cspace }
    }

    fn describe(&self, tool: &str) -> Result<Value, SchemaInspectorError> {
        let cap = self
            .cspace
            .lookup_by_name(tool)
            .ok_or_else(|| SchemaInspectorError::ToolNotFound(tool.to_string()))?;

        let meta = cap.meta();
        let schema = meta
            .tool_schema
            .as_ref()
            .ok_or_else(|| SchemaInspectorError::SchemaMissing(tool.to_string()))?;

        let description = schema
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let input_schema = schema.get("input_schema").cloned().unwrap_or(Value::Null);
        let output_schema = schema.get("output_schema").cloned().unwrap_or(Value::Null);

        Ok(json!({
            "name": meta.name,
            "description": description,
            "input_schema": input_schema,
            "output_schema": output_schema,
            "operations": operation_names(cap.operations()),
        }))
    }
}

fn schema_describe_input(input: &Value) -> Result<&str, SchemaInspectorError> {
    let object = input
        .as_object()
        .ok_or_else(|| SchemaInspectorError::Input("expected a JSON object".to_string()))?;
    if let Some(unknown) = object.keys().find(|k| k.as_str() != "tool") {
        return Err(SchemaInspectorError::Input(format!(
            "unknown field `{unknown}`; the only accepted field is `tool`"
        )));
    }
    object
        .get("tool")
        .and_then(Value::as_str)
        .ok_or_else(|| SchemaInspectorError::Input("expected a `tool` string".to_string()))
}

impl Resource for SchemaInspectorResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let tool = schema_describe_input(&input).map_err(|e| e.to_string())?;
        self.describe(tool).map_err(|e| e.to_string())
    }
}

pub struct SchemaInspectorBuiltin;

impl BuiltinManifest for SchemaInspectorBuiltin {
    type Resource = SchemaInspectorResource;
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("schema_inspector")
            .expose(SCHEMA_DESCRIBE_NAME, SCHEMA_DESCRIBE_CONTRACT)
            .timeout_ms(5000)
            .build()
    }
}

impl SchemaInspectorBuiltin {
    pub fn mint(
        &self,
        factory: &CapabilityFactory,
        plugin: &PluginId,
        decl: &CapabilityDecl,
        kind: CapKind,
        budget: CapabilityBudget,
        _bindings: &[ResolvedBinding],
        _typed_bindings: &TypedBindings,
    ) -> Result<SlotId, MintError> {
        use odyssey::core::rights::rights::{CapabilityRights, Rights};

        let pc = factory.plugin_cspace(plugin);
        let local_slot = pc.mint(
            kind,
            decl,
            budget.clone(),
            Arc::new(SchemaInspectorResource::new(factory.space().clone())),
        );
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<SchemaInspectorResource>(
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

    pub fn register() -> (PluginManifest, MintFn, RuinFn) {
        (
            SchemaInspectorBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                SchemaInspectorBuiltin.mint(
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
}

// ===========================================================================
// Shared helpers
// ===========================================================================

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
