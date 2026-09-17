//! Handler binding tests (Slice 5 follow-up).
//!
//! Closes the gap the original loader left: today, a loaded
//! plugin's `mint_fn` is a placeholder that panics on invoke.
//! After binding, the same loader produces a `mint_fn` that
//! dispatches into a [`HandlerRegistry`] keyed by capability
//! name.
//!
//! Tests:
//! - [`registered_handler_is_invoked_for_bound_capability`]:
//!   the registered handler runs and returns the cspace's first
//!   allocated `SlotId` (the factory's allocator starts at
//!   raw = 1, so a fresh `CapabilityFactory` produces
//!   `SlotId::new(1)` on its first mint).
//! - [`unbound_capability_panics_with_placeholder_message`]:
//!   the registry is installed but doesn't cover the
//!   capability being minted → the bound `mint_fn` panics
//!   (the placeholder path is preserved as a loud failure).
//! - [`placeholder_mint_fn_panics_when_invoked`]:
//!   `load_plugin_from_path` (no registry) returns the
//!   original placeholder `mint_fn`, which still panics.
//!
//! Test isolation: each test uses a unique plugin name so the
//! global dispatch table doesn't cross-contaminate (the table
//! is keyed by `plugin.name`). Two tests can run in parallel
//! without colliding.

use std::io::Write;
use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::core::contract::resource::Resource;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::CapabilityDecl;
use odyssey::personality::composition::resolve::ResolvedBinding;
use odyssey::personality::lifecycle::loader::{
    Handler, HandlerRegistry, load_plugin_from_path, load_plugin_from_path_with_handlers,
};
use odyssey::personality::lifecycle::mint::CapabilityFactory;

// ---------------------------------------------------------------------------
// Stub resource + handler
// ---------------------------------------------------------------------------

/// A `Resource` impl that does nothing on invoke / open. The
/// tests only exercise the mint path, so the default impls
/// (`Err("resource does not support ...")`) are fine — what
/// matters is that the kernel accepted the handler and minted
/// a real `Capability<R>` into the cspace.
#[derive(Debug)]
struct StubResource;

impl Resource for StubResource {}

/// The stub handler. Calls
/// `factory.mint::<StubResource>(...)` with the args the
/// orchestrator hands us, producing a real capability in the
/// factory's cspace and returning the fresh `SlotId`. This
/// is the same shape every concrete `MintFn` has — a
/// non-capturing fn pointer that hardcodes its `R` at the
/// definition site.
fn stub_handler(
    factory: &CapabilityFactory,
    plugin: &PluginId,
    decl: &CapabilityDecl,
    kind: CapKind,
    budget: CapabilityBudget,
    _bindings: &[ResolvedBinding],
) -> SlotId {
    factory.mint::<StubResource>(kind, decl, plugin, budget, Arc::new(StubResource))
}

// ---------------------------------------------------------------------------
// Manifest writer — each test writes its own JSON so plugin
// names stay unique without sharing state across tests.
// ---------------------------------------------------------------------------

fn write_manifest(plugin_name: &str, json: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "odyssey-handler-binding-test-{}-{}",
        plugin_name,
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let path = dir.join(format!("{plugin_name}.manifest.json"));
    let mut f = std::fs::File::create(&path).expect("create file");
    f.write_all(json.as_bytes()).expect("write");
    drop(f);
    path
}

// ---------------------------------------------------------------------------
// Positive: registered handler is invoked
// ---------------------------------------------------------------------------

#[test]
fn registered_handler_is_invoked_for_bound_capability() {
    let manifest_json = r#"{
        "plugin": {"name": "handler_test_a", "version": "0.1.0"},
        "exposes": [{"name": "demo_cap", "kind": "sync", "contract_name": "demo"}]
    }"#;
    let path = write_manifest("handler_test_a", manifest_json);

    // Register the stub handler for the only capability the
    // plugin exposes. `with` takes `impl Into<Handler>` so a
    // bare fn pointer coerces without an explicit cast.
    let registry = Arc::new(HandlerRegistry::new().with("demo_cap", stub_handler));

    let loaded =
        load_plugin_from_path_with_handlers(&path, registry.clone()).expect("load should succeed");

    // `handlers` is informational — the registry is already in
    // the global table by the time the loader returns. The
    // `Some` arm tells the operator "you got what you asked
    // for" without forcing them to introspect via the table.
    assert!(
        loaded.handlers.is_some(),
        "loaded.handlers should be Some when a registry was provided"
    );

    // Mint via the bound mint_fn. We don't call run_on —
    // invoking the mint_fn directly exercises the same path
    // the orchestrator does.
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace);
    let plugin = loaded.manifest.plugin.clone();
    let decl = loaded.manifest.exposes[0].clone();
    let budget = CapabilityBudget::new(5000);

    let slot_id = (loaded.mint_fn)(
        &factory,
        &plugin,
        &decl,
        decl.kind,
        budget,
        &[],
    );

    // Factory's allocator starts at raw = 1; a fresh cspace
    // + first mint produces SlotId::new(1). This confirms
    // the stub handler actually ran (the placeholder would
    // have panicked before returning anything).
    assert_eq!(
        slot_id,
        SlotId::new(1),
        "stub handler should mint into the factory's cspace and return the first SlotId"
    );
}

// ---------------------------------------------------------------------------
// Negative: capability name not in registry → placeholder panic
// ---------------------------------------------------------------------------

#[test]
fn unbound_capability_panics_with_placeholder_message() {
    // The plugin exposes two capabilities; the registry only
    // covers one. When the orchestrator (or this test) calls
    // mint_fn for the unbound name, the loader's bound
    // mint_fn falls into its placeholder branch and panics
    // with the same message shape the original placeholder
    // uses.
    let manifest_json = r#"{
        "plugin": {"name": "handler_test_b", "version": "0.1.0"},
        "exposes": [
            {"name": "demo_cap", "kind": "sync", "contract_name": "demo"},
            {"name": "other_cap", "kind": "sync", "contract_name": "other"}
        ]
    }"#;
    let path = write_manifest("handler_test_b", manifest_json);

    let registry = Arc::new(HandlerRegistry::new().with("demo_cap", stub_handler));

    let loaded =
        load_plugin_from_path_with_handlers(&path, registry).expect("load should succeed");

    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace);
    let plugin = loaded.manifest.plugin.clone();
    // Pick the capability the registry doesn't cover.
    let decl = loaded
        .manifest
        .exposes
        .iter()
        .find(|d| d.name == "other_cap")
        .expect("other_cap should be in manifest")
        .clone();
    let budget = CapabilityBudget::new(5000);

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (loaded.mint_fn)(&factory, &plugin, &decl, decl.kind, budget, &[])
    }));

    assert!(
        outcome.is_err(),
        "mint_fn for an unbound capability must panic — the placeholder contract is loud failure"
    );
}

// ---------------------------------------------------------------------------
// Negative: the original placeholder still panics for callers of
// load_plugin_from_path (no registry installed).
// ---------------------------------------------------------------------------

#[test]
fn placeholder_mint_fn_panics_when_invoked() {
    let manifest_json = r#"{
        "plugin": {"name": "handler_test_c", "version": "0.1.0"},
        "exposes": [{"name": "demo_cap", "kind": "sync", "contract_name": "demo"}]
    }"#;
    let path = write_manifest("handler_test_c", manifest_json);

    let loaded = load_plugin_from_path(&path).expect("load should succeed");
    assert!(
        loaded.handlers.is_none(),
        "load_plugin_from_path should leave handlers=None (no registry installed)"
    );

    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::new(cspace);
    let plugin = loaded.manifest.plugin.clone();
    let decl = loaded.manifest.exposes[0].clone();
    let budget = CapabilityBudget::new(5000);

    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (loaded.mint_fn)(&factory, &plugin, &decl, decl.kind, budget, &[])
    }));

    assert!(
        outcome.is_err(),
        "placeholder mint_fn must panic when invoked; today's callers keep the loud failure"
    );
}

// ---------------------------------------------------------------------------
// HandlerRegistry surface
// ---------------------------------------------------------------------------

#[test]
fn handler_registry_with_chains_insertions() {
    let r = HandlerRegistry::new()
        .with("a", stub_handler as Handler)
        .with("b", stub_handler as Handler);
    assert_eq!(r.len(), 2);
    assert!(r.lookup("a").is_some());
    assert!(r.lookup("b").is_some());
    assert!(r.lookup("missing").is_none());
    assert!(!r.is_empty());
}

#[test]
fn handler_registry_default_is_empty() {
    let r = HandlerRegistry::default();
    assert!(r.is_empty());
    assert_eq!(r.len(), 0);
    assert!(r.lookup("anything").is_none());
}