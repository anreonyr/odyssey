//! Capability metadata — `CapabilityMeta`, `CapabilityAction`,
//! `AuthorityContract`, `Protocol`.
//!
//! Phase 5: split from `capability::types`. The contract vocabulary
//! lives here; the runtime validates against it through
//! `AuthorityContract::operation_for`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::identity::ids::CapabilityId;
use crate::core::identity::ids::PluginId;
use crate::core::quota::quota::QuotaSpec;

/// Static description of a capability. Carried inside every token;
/// surfaced to the HTTP bridge for enumeration.
///
/// Phase 2 extends the surface with `namespace`, `contract`, and
/// `quota` so the HTTP bridge can render a real schema + rate limit
/// and `cspace.enumerate_namespace(prefix)` can return a scoped view.
#[derive(Clone, Debug)]
pub struct CapabilityMeta {
    pub id: CapabilityId,
    pub name: String,
    /// Hierarchical namespace this capability is filed under, e.g.
    /// `"odyssey.model.llama3"` or `"org.example.db.read"`. Empty
    /// string means "root namespace" (legacy single-name caps).
    pub namespace: String,
    /// Phase 3 P3.1 — the contract name this capability
    /// publishes. Set from the manifest's `[[exposes]] contract_name`
    /// and copied verbatim into the runtime meta so the resolver,
    /// HTTP bridge, and any introspection layer can match
    /// `requires[*].contract` against it. Empty string means
    /// "no contract published" (legacy caps; not reachable via
    /// capability injection).
    pub contract_name: String,
    pub plugin: PluginId,
    pub in_type: String,
    pub out_type: String,
    pub streaming: bool,
    pub timeout_ms: u32,
    pub quota: QuotaSpec,
    /// Authority vocabulary (action → OperationRights map).
    /// Phase 3 P3.2 — extracted from the old `contract` field.
    /// `RuleAgent` reads `authority.operation_for(action)` to
    /// translate verbs to bits. Load-bearing at runtime.
    pub authority: AuthorityContract,
    /// Wire-protocol metadata (schemas, description, transport).
    /// Phase 3 P3.2 — split out of the old `contract` field.
    /// Pure metadata; the runtime never validates against it.
    pub protocol: Protocol,
}

// ---------------------------------------------------------------------------
// Contract (Phase 2)
// ---------------------------------------------------------------------------

/// One entry in a capability's published action vocabulary. A
/// cap that wants type-agnostic callers (e.g. `RuleAgent`) to
/// dispatch to it publishes the list of action verbs it accepts
/// along with the `OperationRights` bit a caller must hold to
/// perform each one. The `operation` field is a string ("READ",
/// "WRITE", "EXECUTE", "ADMIN") rather than the bitflag itself
/// so the contract stays JSON-Schema-friendly and the bitflag's
/// object-map serde shape doesn't leak into manifests.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityAction {
    pub name: String,
    pub operation: String,
}

/// Authority vocabulary published by a capability: the set of
/// action verbs a caller may invoke, each tagged with the
/// `OperationRights` bit the caller must hold.
///
/// This is the **only** part of the original
/// `CapabilityContract` that the runtime path actually
/// consulted — the `RuleAgent` reads it via
/// `AuthorityContract::operation_for(action)`. Splitting it
/// out makes "what authority do I need to perform X?" a
/// first-class question answerable from a small focused
/// struct, separate from "what does the wire look like?".
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AuthorityContract {
    /// Per-capability RPC vocabulary. Empty means "no enumerable
    /// action surface" — callers must already know how to talk to
    /// the cap, or the type-agnostic dispatcher will refuse to
    /// route to it.
    #[serde(default)]
    pub actions: Vec<CapabilityAction>,
}

impl AuthorityContract {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Append an action to the vocabulary.
    pub fn with_action(mut self, name: impl Into<String>, operation: impl Into<String>) -> Self {
        self.actions.push(CapabilityAction {
            name: name.into(),
            operation: operation.into(),
        });
        self
    }

    /// Look up the operation bit string published for `action`.
    /// Returns `None` if the action isn't in the cap's vocabulary
    /// — callers (e.g. the type-agnostic `RuleAgent`) treat that
    /// as a "no such method" error.
    pub fn operation_for(&self, action: &str) -> Option<&str> {
        self.actions
            .iter()
            .find(|a| a.name == action)
            .map(|a| a.operation.as_str())
    }
}

/// Wire-protocol metadata published by a capability. **Does
/// not drive dispatch.** `Resource::invoke(Value)` accepts raw
/// JSON and the handler parses internally; this struct
/// describes the surface so external clients (HTTP bridge,
/// OpenAPI generators, JSON-RPC descriptors) can advertise
/// what the cap accepts without modifying the runtime.
///
/// All fields are optional with sensible defaults so an empty
/// `Protocol` means "no advertised metadata":
///
/// - `description`: free-text one-liner; surfaces in docs.
/// - `input_schema` / `output_schema`: JSON Schema (or any
///   dialect that rides along as `serde_json::Value`).
/// - `media_type`: wire encoding. Default empty string means
///   "not advertised"; the HTTP bridge defaults to
///   `application/json` for JSON-typed caps.
/// - `version`: wire format version, independent of the
///   plugin's own version. Bumping protocol version means the
///   byte layout changed, not the capability's behaviour.
/// - `transport`: hint of where this cap is reachable
///   (`in-process`, `http`, `grpc`, ...). Empty means "not
///   advertised". The HTTP bridge uses this to decide
///   whether to register a route; in-process caps skip it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Protocol {
    /// One-line description. Surfaces in HTTP bridge docs and
    /// OpenAPI generators.
    #[serde(default)]
    pub description: String,
    /// JSON Schema (or similar) for the input object.
    #[serde(default)]
    pub input_schema: Value,
    /// JSON Schema (or similar) for the output value.
    #[serde(default)]
    pub output_schema: Value,
    /// Wire encoding. Empty means "not advertised".
    #[serde(default)]
    pub media_type: String,
    /// Wire format version. Independent of the plugin's own
    /// version. Bumping protocol.version means the byte layout
    /// changed, not the capability's behaviour.
    #[serde(default)]
    pub version: String,
    /// Hint of where this cap is reachable (`in-process`,
    /// `http`, `grpc`, ...). Empty means "not advertised".
    #[serde(default)]
    pub transport: String,
}

impl Protocol {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_input(mut self, schema: Value) -> Self {
        self.input_schema = schema;
        self
    }

    pub fn with_output(mut self, schema: Value) -> Self {
        self.output_schema = schema;
        self
    }

    pub fn with_media_type(mut self, mt: impl Into<String>) -> Self {
        self.media_type = mt.into();
        self
    }

    pub fn with_version(mut self, v: impl Into<String>) -> Self {
        self.version = v.into();
        self
    }

    pub fn with_transport(mut self, t: impl Into<String>) -> Self {
        self.transport = t.into();
        self
    }
}
