//! ε — Phase 3 P3.1: Capability Injection.
//!
//! Tests that prove the resolver end-to-end:
//!
//! - **ε.1 — Contract-keyed binding** ([`contract_keyed_binding`]):
//!   a manifest with `[[requires]] contract = "X"` resolves to
//!   the plugin whose `[[exposes]] contract_name = "X"`. The
//!   returned binding carries the provider's `PluginId` and
//!   capability name, regardless of the provider's plugin name
//!   or version.
//! - **ε.2 — Plugin version is invisible to consumers**
//!   ([`plugin_version_swappable`]): two manifests with different
//!   versions but the same `contract_name` are interchangeable
//!   from the consumer's perspective. Resolver succeeds in both
//!   arrangements.
//! - **ε.3 — CapabilityMeta carries contract_name**
//!   ([`contract_name_reaches_meta`]): the contract name declared
//!   in the manifest reaches `slot.meta().contract_name`. This is
//!   what makes the resolver's binding table meaningful at
//!   runtime — without it, the kernel-level cap has no way to
//!   tell what contract it fulfils.
//! - **ε.4 — Unprovided contract is fatal**
//!   ([`unprovided_contract_fails_at_boot`]): a manifest
//!   declaring `[[requires]] contract = "ghost"` fails the
//!   resolver with `ResolveError::Unprovided`.
//! - **ε.5 — Ambiguous contract is fatal**
//!   ([`ambiguous_contract_fails_at_boot`]): two manifests
//!   publishing the same contract_name forces the resolver to
//!   fail with `ResolveError::Ambiguous` — Phase 3 P3.1 has no
//!   priority hint mechanism yet.
//! - **ε.6 — Cycle is fatal**
//!   ([`cycle_detection_at_resolver`]): two plugins requiring
//!   each other's contracts trips `ResolveError::Cycle`.
//! - **ε.7 — Diamond orders correctly**
//!   ([`diamond_dependency_topology`]): a shared provider feeds
//!   two consumers, each of which feeds a fourth. The resolver
//!   puts the provider before both consumers, and both consumers
//!   before the fourth.
//! - **ε.8 — Real runtime plugins still parse**
//!   ([`runtime_manifests_have_contract_names`]): every manifest
//!   under `src/plugins/` declares a non-empty `contract_name`.
//!   This guards against future contributors forgetting the field.

use odyssey::capability::{
    Capability, CapabilityBudget, CapabilitySpace, CapKind, Resource,
};
use odyssey::kernel::factory::CapabilityFactory;
use odyssey::kernel::manifest::{
    CapabilityDecl, CapabilityRequirement, IsolationMode, PluginId, PluginManifest, ResourceHints,
};
use odyssey::kernel::resolver::{resolve, ResolveError, ResolvedBinding, ResolvedPlan};

// ---------------------------------------------------------------------------
// Test fixtures
// ---------------------------------------------------------------------------

fn pid(name: &str, version: &str) -> PluginId {
    PluginId {
        name: name.into(),
        version: version.into(),
    }
}

fn exposes_contract(name: &str, contract: &str) -> CapabilityDecl {
    CapabilityDecl {
        name: name.into(),
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
        contract_name: contract.into(),
        authority: Default::default(),
        protocol: Default::default(),
    }
}

fn exposes_no_contract(name: &str) -> CapabilityDecl {
    CapabilityDecl {
        name: name.into(),
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
        contract_name: String::new(),
        authority: Default::default(),
        protocol: Default::default(),
    }
}

#[allow(dead_code)]
fn _silence_exposes_no_contract() {
    // Keep the helper in the file for symmetry with
    // `exposes_contract` even though P3.1 doesn't have a test
    // that uses it. (ε.7 in resolver::tests covers the empty-
    // contract case at the unit level.) Removing this silencer
    // will make the unused-function warning reappear.
    let _ = exposes_no_contract("noop");
}

fn requires(handle: &str, contract: &str) -> CapabilityRequirement {
    CapabilityRequirement {
        name: handle.into(),
        contract: contract.into(),
    }
}

fn manifest(
    name: &str,
    version: &str,
    exp: Vec<CapabilityDecl>,
    req: Vec<CapabilityRequirement>,
) -> PluginManifest {
    PluginManifest {
        plugin: pid(name, version),
        isolate: IsolationMode::InProc,
        exposes: exp,
        requires: req,
        consumes: Vec::new(),
        host: Vec::new(),
        resources: ResourceHints::default(),
    }
}

fn position_of(plan: &ResolvedPlan, name: &str) -> usize {
    plan.mint_order
        .iter()
        .position(|p| p.name == name)
        .unwrap_or_else(|| panic!("plugin {name} not in plan"))
}

// ---------------------------------------------------------------------------
// ε.1 — Contract-keyed binding
// ---------------------------------------------------------------------------

#[test]
fn contract_keyed_binding() {
    // `provider` exposes contract "gen_cap". `consumer` requires it.
    // The consumer doesn't care that the plugin is called "provider"
    // — only that something publishes "gen_cap".
    let provider = manifest(
        "provider",
        "0.1.0",
        vec![exposes_contract("generate", "gen_cap")],
        vec![],
    );
    let consumer = manifest(
        "consumer",
        "0.1.0",
        vec![exposes_contract("consumer", "consumer_cap")],
        vec![requires("g", "gen_cap")],
    );

    let plan = resolve(&[provider, consumer]).expect("resolve succeeds");

    // Provider must mint before consumer.
    assert!(position_of(&plan, "provider") < position_of(&plan, "consumer"));

    // Consumer's binding points at the provider's plugin + capability.
    let consumer_id = pid("consumer", "0.1.0");
    let bindings: &[ResolvedBinding] = plan
        .bindings
        .get(&consumer_id)
        .expect("consumer has a binding entry");
    assert_eq!(bindings.len(), 1);
    let b = &bindings[0];
    assert_eq!(b.handle, "g");
    assert_eq!(b.contract, "gen_cap");
    assert_eq!(b.provider.name, "provider");
    assert_eq!(b.provider.version, "0.1.0");
    assert_eq!(b.capability, "generate");
}

// ---------------------------------------------------------------------------
// ε.2 — Plugin version is invisible to consumers
// ---------------------------------------------------------------------------

#[test]
fn plugin_version_swappable() {
    // Two scenarios: provider is v0.1.0, then provider is v0.2.0.
    // Both should resolve identically from the consumer's point
    // of view. The consumer never declares a version.
    let consumer = manifest(
        "consumer",
        "0.1.0",
        vec![exposes_contract("c", "c_cap")],
        vec![requires("p", "provider_cap")],
    );

    for version in ["0.1.0", "0.2.0", "1.0.0", "9.9.9"] {
        let provider = manifest(
            "provider",
            version,
            vec![exposes_contract("provide", "provider_cap")],
            vec![],
        );
        let plan = resolve(&[provider, consumer.clone()])
            .unwrap_or_else(|e| panic!("version {version}: resolve failed: {e}"));
        assert_eq!(plan.mint_order.len(), 2);
        let b = &plan.bindings.get(&pid("consumer", "0.1.0")).unwrap()[0];
        assert_eq!(
            b.provider.version, version,
            "version {version} should be the one resolved"
        );
    }
}

// ---------------------------------------------------------------------------
// ε.3 — CapabilityMeta carries contract_name
// ---------------------------------------------------------------------------

#[test]
fn contract_name_reaches_meta() {
    // The resolver is one half of the loop. The other half is
    // the factory: when it mints a typed cap, it must copy the
    // contract name from the manifest's CapabilityDecl into the
    // resulting CapabilityMeta. Otherwise runtime introspection
    // (HTTP bridge, Agent) can't tell what contract a cap
    // fulfils.
    let (cspace, factory) = crate::common::boot();
    let decl = exposes_contract("echo_cap", "echo_contract");
    let plugin = pid("echo", "0.1.0");
    let slot = factory.mint::<Echo>(
        CapKind::Sync,
        &decl,
        &plugin,
        CapabilityBudget::new(5000),
        std::sync::Arc::new(Echo),
    );
    let meta = cspace.slot_meta(slot).expect("slot has meta");
    assert_eq!(meta.contract_name, "echo_contract");
}

// ---------------------------------------------------------------------------
// ε.4 — Unprovided contract fails at boot
// ---------------------------------------------------------------------------

#[test]
fn unprovided_contract_fails_at_boot() {
    let consumer = manifest(
        "consumer",
        "0.1.0",
        vec![exposes_contract("c", "c_cap")],
        vec![requires("ghost", "no_such_contract")],
    );
    let err = resolve(&[consumer]).unwrap_err();
    match err {
        ResolveError::Unprovided { contract, by } => {
            assert_eq!(contract, "no_such_contract");
            assert_eq!(by, "consumer");
        }
        other => panic!("expected Unprovided, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// ε.5 — Ambiguous contract fails at boot
// ---------------------------------------------------------------------------

#[test]
fn ambiguous_contract_fails_at_boot() {
    // Two providers both publish "shared". Without a priority
    // hint (not in Phase 3 P3.1), the resolver can't pick one.
    let a = manifest("a", "0.1.0", vec![exposes_contract("a_cap", "shared")], vec![]);
    let b = manifest("b", "0.1.0", vec![exposes_contract("b_cap", "shared")], vec![]);
    let c = manifest(
        "consumer",
        "0.1.0",
        vec![exposes_contract("c", "c_cap")],
        vec![requires("s", "shared")],
    );
    let err = resolve(&[a, b, c]).unwrap_err();
    match err {
        ResolveError::Ambiguous { contract, a, b } => {
            assert_eq!(contract, "shared");
            assert!(a.starts_with("a@") || a.starts_with("b@"));
            assert!(b.starts_with("a@") || b.starts_with("b@"));
        }
        other => panic!("expected Ambiguous, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// ε.6 — Cycle detection
// ---------------------------------------------------------------------------

#[test]
fn cycle_detection_at_resolver() {
    let a = manifest(
        "a",
        "0.1.0",
        vec![exposes_contract("a", "a_contract")],
        vec![requires("b", "b_contract")],
    );
    let b = manifest(
        "b",
        "0.1.0",
        vec![exposes_contract("b", "b_contract")],
        vec![requires("a", "a_contract")],
    );
    let err = resolve(&[a, b]).unwrap_err();
    match err {
        ResolveError::Cycle { chain } => {
            assert_eq!(chain.len(), 2);
            assert!(chain.iter().any(|s| s.starts_with("a@")));
            assert!(chain.iter().any(|s| s.starts_with("b@")));
        }
        other => panic!("expected Cycle, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// ε.7 — Diamond dependency orders correctly
// ---------------------------------------------------------------------------

#[test]
fn diamond_dependency_topology() {
    // a is a shared provider.
    // b and c both require a.
    // d requires both b and c.
    let a = manifest("a", "0.1.0", vec![exposes_contract("a_cap", "a_contract")], vec![]);
    let b = manifest(
        "b",
        "0.1.0",
        vec![exposes_contract("b_cap", "b_contract")],
        vec![requires("a", "a_contract")],
    );
    let c = manifest(
        "c",
        "0.1.0",
        vec![exposes_contract("c_cap", "c_contract")],
        vec![requires("a", "a_contract")],
    );
    let d = manifest(
        "d",
        "0.1.0",
        vec![exposes_contract("d_cap", "d_contract")],
        vec![
            requires("b", "b_contract"),
            requires("c", "c_contract"),
        ],
    );
    let plan = resolve(&[a, b, c, d]).unwrap();

    assert!(position_of(&plan, "a") < position_of(&plan, "b"));
    assert!(position_of(&plan, "a") < position_of(&plan, "c"));
    assert!(position_of(&plan, "b") < position_of(&plan, "d"));
    assert!(position_of(&plan, "c") < position_of(&plan, "d"));

    // d receives both bindings.
    let d_id = pid("d", "0.1.0");
    let db = plan.bindings.get(&d_id).unwrap();
    assert_eq!(db.len(), 2);
    let handles: Vec<&str> = db.iter().map(|b| b.handle.as_str()).collect();
    assert!(handles.contains(&"b"));
    assert!(handles.contains(&"c"));
}

// ---------------------------------------------------------------------------
// ε.8 — Real runtime + test_only plugins still have contract_name
// ---------------------------------------------------------------------------

#[test]
fn runtime_manifests_have_contract_names() {
    // Guard against regressions where a contributor edits a
    // manifest but forgets the `contract_name` field. Every
    // plugin — whether the manifest comes from a `.toml` file
    // (test_only/ subtree) or from a Rust `manifest()` function
    // (runtime plugins under `src/plugins/echo/{basic,chain,stream}/`,
    // `generator/`, `reverse/`, `sandbox/`, `slow/`) — must
    // declare at least one non-empty contract name.
    use std::sync::OnceLock;

    /// Runtime plugins use the Phase 3 manifest-as-Rust-const
    /// pattern. Each plugin exposes a `manifest()` function
    /// returning `&'static PluginManifest`.
    fn collect_runtime_manifests() -> Vec<PluginManifest> {
        vec![
            odyssey::plugins::echo::basic::manifest().clone(),
            odyssey::plugins::echo::chain::manifest().clone(),
            odyssey::plugins::echo::stream::manifest().clone(),
            odyssey::plugins::generator::manifest().clone(),
            odyssey::plugins::reverse::manifest().clone(),
            odyssey::plugins::sandbox::manifest().clone(),
            odyssey::plugins::slow::manifest().clone(),
        ]
    }

    /// Test-only plugins still use the toml wire format — they're
    /// test fixtures that exercise the manifest parser. The
    /// walker skips `test_only/`'s parents because the runtime
    /// boot path doesn't load them; the test still walks the
    /// whole tree because every plugin should be self-consistent.
    fn load_toml_manifests() -> &'static Vec<PluginManifest> {
        static CACHE: OnceLock<Vec<PluginManifest>> = OnceLock::new();
        CACHE.get_or_init(|| {
            fn walk(out: &mut Vec<PluginManifest>, p: &std::path::Path) {
                for entry in std::fs::read_dir(p).unwrap() {
                    let entry = entry.unwrap();
                    let path = entry.path();
                    if entry.file_type().unwrap().is_dir() {
                        walk(out, &path);
                    } else if path.extension().and_then(|s| s.to_str()) == Some("toml") {
                        let m = PluginManifest::from_path(&path)
                            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                        out.push(m);
                    }
                }
            }
            let mut out = Vec::new();
            walk(&mut out, std::path::Path::new("src/plugins/test_only"));
            out
        })
    }

    let mut manifests = collect_runtime_manifests();
    manifests.extend(load_toml_manifests().iter().cloned());

    assert!(
        manifests.len() >= 11,
        "expected ≥11 manifests (7 runtime + 4 test_only), got {}",
        manifests.len()
    );

    for m in manifests.iter() {
        let with_contract: Vec<&CapabilityDecl> = m
            .exposes
            .iter()
            .filter(|c| !c.contract_name.is_empty())
            .collect();
        assert!(
            !with_contract.is_empty(),
            "manifest {}@{} has no capability with a non-empty contract_name",
            m.plugin.name,
            m.plugin.version,
        );
    }

    // And the resolver should be able to handle the whole set.
    let plan = resolve(&manifests).expect("runtime manifests resolve cleanly");
    assert_eq!(plan.mint_order.len(), manifests.len());
}

// ---------------------------------------------------------------------------
// Test resource: a minimal `Resource` impl for ε.3.
// ---------------------------------------------------------------------------

struct Echo;

impl Resource for Echo {
    fn invoke(
        &self,
        input: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        Ok(input)
    }
}

// Suppress dead_code warnings on imports we use only on some
// tests; keeps the file's use list honest about cross-test
// dependencies.
#[allow(dead_code)]
type _Space = CapabilitySpace;
#[allow(dead_code)]
type _Factory = CapabilityFactory;
#[allow(dead_code)]
type _Cap<R> = Capability<R>;
