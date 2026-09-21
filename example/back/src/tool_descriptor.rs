//! Tool descriptor builtin — answers "what is this tool's input/output
//! contract?" by reading the cap's `meta.tool_schema` field.
//!
//! The kernel does not know what JSON Schema means. The field is
//! carried as opaque `serde_json::Value`. This builtin is the only
//! reader; everything else (the agent, a UI, a test) goes through
//! `tool_describe` to learn a tool's contract.
//!
//! ## Why this is its own plugin
//!
//! The principle the agent design rests on: an AI agent is composed
//! of multiple capabilities, each doing one thing. Tool description
//! is one of those things. Bundling it into the agent's own code
//! would mean the agent introspects other caps directly; making it
//! a peer plugin means anyone (the agent, another agent, a future
//! `agent_describe` UI) can use the same `tool_describe` capability.
//!
//! ## Input / output
//!
//! ```text
//! in:  { "tool": "<capability_name>" }
//! out: { "name": "...", "description": "...",
//!        "input_schema": <JSON Schema>, "output_schema": <JSON Schema>,
//!        "operations": ["READ", "EXECUTE", ...] }
//! ```
//!
//! Errors:
//! - `Input` — input shape wrong (not an object, missing `tool`).
//! - `ToolNotFound` — no cap by that name in the cspace.
//! - `SchemaMissing` — the cap exists but didn't publish a
//!   `tool_schema` in its manifest.

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

/// The contract name this capability publishes.
pub const CONTRACT: &str = "tool_descriptor";
/// The capability name (also the cspace name).
pub const NAME: &str = "tool_describe";

#[derive(Debug, PartialEq, Eq)]
pub enum ToolDescriptorError {
    /// Input was not the documented shape.
    Input(String),
    /// No cap by this name exists in the cspace.
    ToolNotFound(String),
    /// The cap exists but didn't publish a `tool_schema`. The agent
    /// cannot use this tool in a schema-driven way; it can still
    /// call it by name through `cspace.lookup_by_name`, just
    /// without structured input/output metadata.
    SchemaMissing(String),
}

impl std::fmt::Display for ToolDescriptorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Input(message) => write!(f, "tool_descriptor: {message}"),
            Self::ToolNotFound(name) => {
                write!(f, "tool_descriptor: tool `{name}` not found in cspace")
            }
            Self::SchemaMissing(name) => write!(
                f,
                "tool_descriptor: tool `{name}` exists but has no `tool_schema` declared"
            ),
        }
    }
}

pub struct ToolDescriptorResource {
    cspace: CapabilitySpace,
}

impl ToolDescriptorResource {
    pub fn new(cspace: CapabilitySpace) -> Self {
        Self { cspace }
    }

    fn describe(&self, tool: &str) -> Result<Value, ToolDescriptorError> {
        let cap = self
            .cspace
            .lookup_by_name(tool)
            .ok_or_else(|| ToolDescriptorError::ToolNotFound(tool.to_string()))?;

        let meta = cap.meta();
        let schema = meta
            .tool_schema
            .as_ref()
            .ok_or_else(|| ToolDescriptorError::SchemaMissing(tool.to_string()))?;

        // The schema is an opaque JSON object the manifest author
        // chose. We surface a few conventional fields if present
        // and pass the rest through under `schema` for forward
        // compatibility. Authors can stash anything they like.
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

/// Closed input shape — accepts only the documented `tool` field.
/// The same principle as `agent_describe`: an unknown field is
/// refused rather than dropped, so a caller who believes the
/// descriptor dispatches the tool can't read a description as
/// evidence the call happened.
fn describe_input(input: &Value) -> Result<&str, ToolDescriptorError> {
    let object = input
        .as_object()
        .ok_or_else(|| ToolDescriptorError::Input("expected a JSON object".to_string()))?;
    if let Some(unknown) = object.keys().find(|k| k.as_str() != "tool") {
        return Err(ToolDescriptorError::Input(format!(
            "unknown field `{unknown}`; the only accepted field is `tool`"
        )));
    }
    object
        .get("tool")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolDescriptorError::Input("expected a `tool` string".to_string()))
}

impl Resource for ToolDescriptorResource {
    fn invoke(&self, input: Value) -> Result<Value, String> {
        let tool = describe_input(&input).map_err(|e| e.to_string())?;
        self.describe(tool).map_err(|e| e.to_string())
    }
}

pub struct ToolDescriptorBuiltin;

impl BuiltinManifest for ToolDescriptorBuiltin {
    type Resource = ToolDescriptorResource;
    fn manifest(&self) -> PluginManifest {
        ManifestBuilder::new("tool_descriptor")
            .expose(NAME, CONTRACT)
            .timeout_ms(5000)
            .build()
    }
}

impl ToolDescriptorBuiltin {
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
            Arc::new(ToolDescriptorResource::new(factory.space().clone())),
        );
        let rights = CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: budget.timeout_ms(),
        };
        pc.inner()
            .grant_to::<ToolDescriptorResource>(
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
            ToolDescriptorBuiltin.manifest(),
            |factory, plugin, decl, kind, budget, bindings, typed_bindings| {
                ToolDescriptorBuiltin.mint(
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
