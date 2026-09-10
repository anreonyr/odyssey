//! Builder for [`PluginManifest`] — Phase 3 manifest template.
//!
//! Replaces the toml parser + the 50-line struct-literal each
//! plugin used to write. Each runtime plugin's `manifest.rs`
//! becomes ~7 lines of fluent builder calls. Defaults fill in
//! the convention:
//!
//! | Field          | Default               |
//! |----------------|-----------------------|
//! | `version`      | `"0.1.0"`             |
//! | `in_type`      | `"any"`               |
//! | `out_type`     | `"any"`               |
//! | `streaming`    | `false`               |
//! | `requires`     | `[]`                  |
//! | `consumes`     | `[]`                  |
//! | `host`         | `[]`                  |
//! | `timeout_ms`   | `None` → host default |
//!
//! # Why a builder, not a macro
//!
//! macro_rules! ran into depth-restriction problems: every
//! optional field (`requires`, `host`, `timeout_ms`, plus the
//! nested optional fields inside `expose: { ... }`) is its
//! own `$()` repetition, and macro_rules! requires the
//! metavariable's depth in the input to match its depth in
//! the expansion. With four overlapping optionals the
//! nesting adds up faster than the rules allow.
//!
//! A fluent builder sidesteps the depth issue entirely: each
//! call is at depth 0, defaults are filled in by `new()`, and
//! missing fields stay at their default until a setter is
//! called.
//!
//! # Why not a proc-macro derive
//!
//! A proc-macro derive needs a separate `odyssey-macros` crate
//! (Rust won't let proc-macros live in the same crate they
//! expand into). It also forces the user to declare an empty
//! struct per plugin (`struct EchoManifest;`) just to hang
//! attributes off of. The builder is one less indirection.
//!
//! # Example
//!
//! ```ignore
//! use crate::kernel::manifest_builder::ManifestBuilder;
//!
//! pub fn manifest() -> &'static PluginManifest {
//!     static M: OnceLock<PluginManifest> = OnceLock::new();
//!     M.get_or_init(|| {
//!         ManifestBuilder::new("echo", "echo", "echo")
//!             .host("dispatcher")
//!             .timeout_ms(5000)
//!             .build()
//!     })
//! }
//! ```

use crate::capability::AuthorityContract;
use crate::kernel::manifest::{
    CapabilityDecl, CapabilityRequirement, HostServiceRef, IsolationMode, PluginId,
    PluginManifest, ResourceHints,
};

/// Fluent builder for [`PluginManifest`]. Construct via
/// [`ManifestBuilder::new`]; chain `.host()`, `.requires()`,
/// `.timeout_ms()`, etc. to override defaults; call `.build()`
/// to materialise the manifest.
///
/// The builder is a *value type* — each setter consumes `self`
/// and returns `self`. Callers therefore write linear chains
/// rather than mutating a shared state.
pub struct ManifestBuilder {
    name: String,
    version: String,
    cap_name: String,
    cap_contract: String,
    cap_in_type: String,
    cap_out_type: String,
    cap_streaming: bool,
    requires: Vec<CapabilityRequirement>,
    consumes: Vec<crate::kernel::manifest::DependencyRef>,
    host: Vec<HostServiceRef>,
    timeout_ms: Option<u32>,
    actions: Vec<(String, String)>,
    protocol: crate::capability::Protocol,
}

impl ManifestBuilder {
    /// Start a manifest for a plugin named `plugin_name`,
    /// exposing one capability whose name and contract are
    /// `cap_name` and `cap_contract`.
    pub fn new(
        plugin_name: impl Into<String>,
        cap_name: impl Into<String>,
        cap_contract: impl Into<String>,
    ) -> Self {
        Self {
            name: plugin_name.into(),
            version: "0.1.0".into(),
            cap_name: cap_name.into(),
            cap_contract: cap_contract.into(),
            cap_in_type: "any".into(),
            cap_out_type: "any".into(),
            cap_streaming: false,
            requires: Vec::new(),
            consumes: Vec::new(),
            host: Vec::new(),
            timeout_ms: None,
            actions: Vec::new(),
            protocol: crate::capability::Protocol::empty(),
        }
    }

    /// Override the plugin version (default `"0.1.0"`).
    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = v.into();
        self
    }

    /// Override the capability's input type (default `"any"`).
    pub fn in_type(mut self, t: impl Into<String>) -> Self {
        self.cap_in_type = t.into();
        self
    }

    /// Override the capability's output type (default `"any"`).
    pub fn out_type(mut self, t: impl Into<String>) -> Self {
        self.cap_out_type = t.into();
        self
    }

    /// Mark the capability as streaming (default `false`).
    pub fn streaming(mut self, s: bool) -> Self {
        self.cap_streaming = s;
        self
    }

    /// Append a `[[requires]]` entry. May be called multiple
    /// times to declare multiple capability-keyed dependencies.
    pub fn requires(
        mut self,
        handle: impl Into<String>,
        contract: impl Into<String>,
    ) -> Self {
        self.requires.push(CapabilityRequirement {
            name: handle.into(),
            contract: contract.into(),
        });
        self
    }

    /// Append a legacy `[[consumes]]` entry. Plugin-version-keyed
    /// dependency kept around for back-compat with the boot
    /// validation log (which still prints the "plugin X consumes
    /// Y from plugin Z@version" line for any non-empty `consumes`).
    /// New plugins should prefer `.requires()` (capability-keyed);
    /// the resolver walks `requires`, not `consumes`.
    pub fn consumes(
        mut self,
        plugin: impl Into<String>,
        version: impl Into<String>,
        capability: impl Into<String>,
    ) -> Self {
        self.consumes.push(crate::kernel::manifest::DependencyRef {
            plugin: plugin.into(),
            version: version.into(),
            capability: capability.into(),
        });
        self
    }

    /// Append a `[[host]]` entry. May be called multiple times
    /// to declare multiple host services.
    pub fn host(mut self, service: impl Into<String>) -> Self {
        self.host.push(HostServiceRef {
            service: service.into(),
            scope: None,
        });
        self
    }

    /// Append a `[[exposes.authority.actions]]` entry — publishes
    /// the action → `OperationRights` mapping that
    /// type-agnostic dispatchers (RuleAgent) consult. Call once
    /// per action. The authority defaults to `empty()` if never
    /// called.
    pub fn action(
        mut self,
        name: impl Into<String>,
        operation: impl Into<String>,
    ) -> Self {
        // Stash action name + operation on the builder; we'll
        // fold them into an `AuthorityContract` at `build()`.
        self.actions.push((
            name.into(),
            operation.into(),
        ));
        self
    }

    /// Set the wire-protocol metadata in one call. Phase 3
    /// P3.2 — the manifest's `protocol` block is pure
    /// metadata (schemas, description, transport, version,
    /// media_type); it doesn't affect dispatch.
    ///
    /// Use `.with_description(...)`, `.with_input(...)`,
    /// `.with_output(...)`, `.with_media_type(...)`,
    /// `.with_version(...)`, `.with_transport(...)` on the
    /// returned `Protocol` to set individual fields.
    pub fn protocol(mut self, p: crate::capability::Protocol) -> Self {
        self.protocol = p;
        self
    }

    /// Set the timeout hint in milliseconds (default `None`,
    /// which lets `CapabilityBudget::new` use the host default).
    pub fn timeout_ms(mut self, ms: u32) -> Self {
        self.timeout_ms = Some(ms);
        self
    }

    /// Materialise the manifest. Consumes the builder.
    pub fn build(self) -> PluginManifest {
        let ManifestBuilder {
            name,
            version,
            cap_name,
            cap_contract,
            cap_in_type,
            cap_out_type,
            cap_streaming,
            requires,
            consumes,
            host,
            timeout_ms,
            actions,
            protocol,
        } = self;
        let mut authority = AuthorityContract::empty();
        for (a, op) in actions {
            authority = authority.with_action(a, op);
        }
        PluginManifest {
            plugin: PluginId { name, version },
            isolate: IsolationMode::InProc,
            exposes: vec![CapabilityDecl {
                name: cap_name,
                in_type: cap_in_type,
                out_type: cap_out_type,
                streaming: cap_streaming,
                contract_name: cap_contract,
                authority,
                protocol,
            }],
            requires,
            consumes,
            host,
            resources: ResourceHints {
                timeout_ms,
                ..Default::default()
            },
        }
    }
}
