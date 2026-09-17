//! Plugin manifest loader (Slice 5 of Direction A).
//!
//! Reads a plugin manifest from disk, validates it, and produces
//! a `(PluginManifest, MintFn, RuinFn)` triple that can be passed
//! to `run_on`.
//!
//! The on-disk format is JSON (serde-derived from the
//! `PluginManifest` shape). The Phase 9 comment in
//! `manifest.rs` noted that a future WASM / cdylib loader
//! could re-attach TOML by adding the `toml` dep and
//! calling `serde::Deserialize` on the same struct shape —
//! the schema is decoupled from the on-disk format. For
//! now, JSON avoids a new dep while exercising the same
//! parsing + validation path that a TOML loader would.
//!
//! ## Handler binding (Slice 5 follow-up)
//!
//! A loaded plugin needs a real handler behind each capability
//! it exposes — the `Resource` impl that the kernel mints and
//! installs into the cspace. Slice 5 established the parser +
//! validator + loader wiring; the placeholder `mint_fn` it
//! returned panicked on invoke so the missing binding was
//! loud. This follow-up closes the gap:
//!
//! - [`HandlerRegistry`] maps each capability `name` to a
//!   [`Handler`]. A handler is the same shape as the kernel's
//!   [`MintFn`] — a non-capturing fn pointer that mints a
//!   capability into the factory's cspace. (We deliberately
//!   keep `Handler = MintFn` rather than introducing an
//!   `Arc<dyn Resource>`-returning variant: changing the type
//!   would force the kernel to add a type-erased mint path,
//!   and the orchestrator's contract is fixed at `MintFn`.)
//! - [`load_plugin_from_path_with_handlers`] installs a
//!   registry into a global dispatch table keyed by the
//!   plugin's `name`. The loader's `mint_fn` field is then
//!   set to [`bound_mint_fn`], which at invocation time
//!   looks up the registry by plugin name and dispatches to
//!   the handler that matches the manifest declaration's
//!   `name`.
//! - [`load_plugin_from_path`] is unchanged — it returns a
//!   `LoadedPlugin` whose `mint_fn` is [`placeholder_mint_fn`].
//!   Invoking it still panics with the same message, so today's
//!   callers (operators who haven't wired a registry yet) keep
//!   seeing the loud failure rather than a silent no-op.
//!
//! ### Why a global dispatch table
//!
//! The orchestrator calls `mint_fn` as a `fn(...) -> SlotId`
//! pointer with six positional arguments — none of which
//! carry user data. The loader has no access to the
//! `CapabilityFactory` at load time, so the registry can't be
//! hung off it. We can't change `MintFn`'s shape (the spec is
//! explicit). A `Box<dyn Fn(...)>` for `mint_fn` would be the
//! type-safe answer, but `MintFn = fn(...)` is the contract.
//!
//! Scoping the table by `plugin_name` is natural: the
//! orchestrator already enforces unique plugin names via
//! `plugin_registry`'s "duplicate plugin name" check, and a
//! `HandlerRegistry` is built per loaded plugin. Tests use
//! unique plugin names (`handler_test_a`, `handler_test_b`)
//! for isolation. The table outlives any single load — that
//! matches the loaded plugin's lifetime, since a registry
//! without its plugin would be dead state.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, OnceLock, RwLock};

use crate::core::identity::ids::SlotId;
use crate::core::manifest::manifest::{ManifestLoadError, PluginManifest};
use crate::personality::lifecycle::run::{MintFn, RuinFn, default_ruin};

// ---------------------------------------------------------------------------
// HandlerRegistry — per-plugin name → MintFn dispatch table
// ---------------------------------------------------------------------------

/// A handler is a [`MintFn`] — the same fn-pointer shape the
/// orchestrator dispatches through. Each entry in a
/// [`HandlerRegistry`] is generated at the call site where the
/// concrete `Resource` impl is in scope, so the handler
/// hardcodes the `R` for `factory.mint::<R>(...)` at its
/// definition point.
///
/// We don't introduce a separate `fn(...) -> Arc<dyn Resource>`
/// alias here: changing the type would force the kernel to
/// add a type-erased mint path, and the orchestrator's
/// contract is fixed at `MintFn`. Keeping `Handler = MintFn`
/// means a handler is just a `MintFn` and the registry is
/// `name -> MintFn` — one indirection layer, no new types.
pub type Handler = MintFn;

/// A name-keyed dispatch table for one plugin's capability
/// handlers. Operators compose a registry before calling
/// [`load_plugin_from_path_with_handlers`]: each capability
/// the plugin `exposes` must have a matching entry here, or
/// the loader's bound [`MintFn`] panics at invoke time (the
/// placeholder's loud-failure contract, preserved).
///
/// The registry is built by chaining `.with(name, handler)`
/// calls. Handlers are coerced to [`Handler`] (= [`MintFn`])
/// fn pointers — capturing closures don't fit, by construction.
#[derive(Default, Debug)]
pub struct HandlerRegistry {
    by_name: HashMap<String, Handler>,
}

impl HandlerRegistry {
    /// Empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a handler for `name`. Returns `self` so calls
    /// chain fluently.
    ///
    /// Takes [`Handler`] directly. A bare fn pointer
    /// (`with("cap", stub_mint)`) coerces to `Handler`
    /// without an explicit cast; capturing closures don't
    /// coerce, which is the point — non-capturing only.
    /// We don't take `impl Into<Handler>` because fn-pointer
    /// HRTB lifetimes make the conversion inference-fragile
    /// in trait-bound position; the direct `Handler`
    /// parameter is the simplest path that compiles.
    pub fn with(mut self, name: &str, handler: Handler) -> Self {
        self.by_name.insert(name.to_string(), handler);
        self
    }

    /// Look up a handler by capability name.
    pub fn lookup(&self, name: &str) -> Option<Handler> {
        self.by_name.get(name).copied()
    }

    /// Number of registered handlers. Tests use it as a sanity
    /// check; production code doesn't need it.
    pub fn len(&self) -> usize {
        self.by_name.len()
    }

    /// True when no handlers are registered.
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Global dispatch table — plugin_name -> Arc<HandlerRegistry>
// ---------------------------------------------------------------------------

/// Per-process dispatch table. Keyed by plugin name (the
/// orchestrator's `plugin_registry` already enforces unique
/// plugin names) so loading two different plugins never
/// collides. See the module-level "Why a global dispatch
/// table" note for the constraint that pushed this design.
static PLUGIN_HANDLERS: OnceLock<RwLock<HashMap<String, Arc<HandlerRegistry>>>> =
    OnceLock::new();

fn plugin_handlers() -> &'static RwLock<HashMap<String, Arc<HandlerRegistry>>> {
    PLUGIN_HANDLERS.get_or_init(|| RwLock::new(HashMap::new()))
}

// ---------------------------------------------------------------------------
// LoadedPlugin
// ---------------------------------------------------------------------------

/// A plugin loaded from an on-disk manifest.
///
/// `mint_fn` is either [`placeholder_mint_fn`] (no handlers
/// registered — invoking it panics with the Slice 5 message)
/// or [`bound_mint_fn`] (a real handler registry was passed
/// to [`load_plugin_from_path_with_handlers`] — invoking it
/// dispatches into the registry).
///
/// `handlers` is the registry that produced the bound
/// `mint_fn`. It's informational — the registry is already
/// installed in the global dispatch table by the loader, so
/// dropping the `LoadedPlugin` doesn't unregister handlers.
/// (An explicit `unregister` would be a future addition if
/// hot-reload lands.)
#[derive(Debug)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub mint_fn: MintFn,
    pub ruin_fn: RuinFn,
    /// The registry the loader installed for this plugin, if
    /// any. `None` for [`load_plugin_from_path`], which uses
    /// the placeholder `mint_fn`.
    pub handlers: Option<Arc<HandlerRegistry>>,
}

// ---------------------------------------------------------------------------
// Loaders
// ---------------------------------------------------------------------------

/// Load a plugin from an on-disk manifest. Reads the file,
/// parses it as JSON, validates the manifest's invariants,
/// and produces a `LoadedPlugin` whose `mint_fn` is the
/// placeholder (panics on invoke) and `ruin_fn` is
/// [`default_ruin`].
///
/// The placeholder is the documented behaviour: today's
/// callers — operators who haven't wired a
/// [`HandlerRegistry`] yet — keep seeing the loud failure
/// rather than a silent no-op when a capability is invoked.
pub fn load_plugin_from_path(path: &Path) -> Result<LoadedPlugin, ManifestLoadError> {
    let manifest = PluginManifest::from_path(path)?;
    Ok(LoadedPlugin {
        manifest,
        mint_fn: placeholder_mint_fn,
        ruin_fn: default_ruin,
        handlers: None,
    })
}

/// Load a plugin from an on-disk manifest with a real
/// handler registry installed. The registry is keyed by
/// capability `name`; for each entry the manifest declares
/// under `exposes`, the loader expects a matching handler.
///
/// Behaviour:
/// - The loader reads + validates the manifest (same path as
///   [`load_plugin_from_path`]).
/// - The registry is installed into the global dispatch
///   table under the plugin's `name`. The entry is keyed by
///   plugin name so loading two different plugins never
///   collides; loading the same plugin twice silently
///   overwrites the prior entry (the operator owns that
///   lifecycle).
/// - The returned `LoadedPlugin`'s `mint_fn` is
///   [`bound_mint_fn`]. At invoke time it looks up the
///   registry by `plugin.name` and dispatches into the
///   handler for the current `decl.name`. A capability
///   name that has no handler panics with the placeholder
///   message — the loud-failure contract is preserved when
///   the operator forgets a registration.
///
/// `RuinFn` stays [`default_ruin`]: revoke-by-slot-id is
/// orthogonal to handler binding and Slice 5 established it
/// as the default.
pub fn load_plugin_from_path_with_handlers(
    path: &Path,
    handlers: Arc<HandlerRegistry>,
) -> Result<LoadedPlugin, ManifestLoadError> {
    let manifest = PluginManifest::from_path(path)?;
    let plugin_name = manifest.plugin.name.clone();
    {
        let mut table = plugin_handlers()
            .write()
            .expect("plugin handler registry poisoned");
        table.insert(plugin_name, handlers.clone());
    }
    Ok(LoadedPlugin {
        manifest,
        mint_fn: bound_mint_fn,
        ruin_fn: default_ruin,
        handlers: Some(handlers),
    })
}

// ---------------------------------------------------------------------------
// Mint functions
// ---------------------------------------------------------------------------

/// The bound `MintFn`. Installed by
/// [`load_plugin_from_path_with_handlers`].
///
/// At invoke time:
/// 1. Look up the registry by `plugin.name` in the global
///    dispatch table.
/// 2. Inside the registry, look up the handler by
///    `decl.name`.
/// 3. If both lookups succeed, dispatch into the handler
///    with the same arguments the orchestrator passed.
///
/// Both lookups are pure reads. Lock contention is bounded
/// by the registry's `RwLock`; the global table is shared
/// across all loaded plugins, but reads are short (one
/// `HashMap` probe each).
///
/// The "missing" branches panic with the placeholder
/// message: keeping the loud-failure contract intact means
/// an operator who forgets a handler sees the same error
/// shape regardless of which loader they used.
fn bound_mint_fn(
    factory: &crate::personality::lifecycle::mint::CapabilityFactory,
    plugin: &crate::core::identity::ids::PluginId,
    decl: &crate::core::manifest::manifest::CapabilityDecl,
    kind: crate::core::identity::kind::CapKind,
    budget: crate::capability::enforce::quota::CapabilityBudget,
    bindings: &[crate::personality::composition::resolve::ResolvedBinding],
) -> SlotId {
    let table = plugin_handlers()
        .read()
        .expect("plugin handler registry poisoned");
    let Some(registry) = table.get(plugin.name.as_str()) else {
        panic!(
            "Slice 5 placeholder MintFn invoked: plugin `{}` has no handler \
             registry installed (missing load_plugin_from_path_with_handlers?). \
             A follow-up commit should map each exposed capability name to \
             a real handler.",
            plugin.name
        );
    };
    match registry.lookup(decl.name.as_str()) {
        Some(handler) => handler(factory, plugin, decl, kind, budget, bindings),
        None => panic!(
            "Slice 5 placeholder MintFn invoked: capability `{}` has no \
             handler bound for plugin `{}`. A follow-up commit should map \
             each exposed capability name to a real handler.",
            decl.name, plugin.name
        ),
    }
}

/// The placeholder `MintFn`. When invoked it panics with
/// the Slice 5 message so the operator sees the gap.
///
/// We deliberately don't panic with `HandlerUnbound` —
/// `MintFn` returns `SlotId`, not `Result<SlotId, _>`, and
/// the orchestrator has no error path for a missing
/// handler. A panic is loud and unmissable; an `Err` would
/// have required changing the orchestrator's signature,
/// which the spec forbids.
fn placeholder_mint_fn(
    _factory: &crate::personality::lifecycle::mint::CapabilityFactory,
    _plugin: &crate::core::identity::ids::PluginId,
    _decl: &crate::core::manifest::manifest::CapabilityDecl,
    _kind: crate::core::identity::kind::CapKind,
    _budget: crate::capability::enforce::quota::CapabilityBudget,
    _bindings: &[crate::personality::composition::resolve::ResolvedBinding],
) -> SlotId {
    panic!(
        "Slice 5 placeholder MintFn invoked: loaded plugins don't have handlers bound yet. \
         A follow-up commit should map each exposed capability name to a real handler."
    );
}