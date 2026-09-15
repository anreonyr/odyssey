//! Smoke test — builtins successfully round-trip through the
//! orchestrator's typed mint API.
//!
//! Phase 9.5: replaced the previous `examples/basic.rs::demo_
//! typed_slot` helper, which lived next to the production
//! boot path and could silently drift from it. This test
//! is a CI gate: any change to the orchestrator's mint
//! surface (factory signature, `Slot<R>` construction,
//! builtin's `MintFn` function-pointer shape) must keep
//! this round-trip working, or the test fails at compile /
//! runtime.
//!
//! The check covers every integration point of the typed
//! mint path:
//!
//! - `CapabilitySpace::new()` — cspace construction
//! - `CapabilityFactory::with_clock(cspace, clock)` —
//!   factory setup
//! - `BuiltinManifest::manifest()` — manifest retrieval
//! - `EchoBuiltin::mint(factory, plugin, decl, kind, budget)`
//!   — the inherent typed mint method (Phase 10: the `Mint`
//!   trait surface is gone; builtins call `factory.mint(...)`
//!   directly via a non-capturing closure registered in
//!   the orchestrator's `(PluginManifest, MintFn)` registry)
//! - `Slot::new(cspace, slot_id)` — typed-slot construction
//! - `Slot::invoke(input)` — sync dispatch returning
//!   `Result<Value, CapabilityError>`
//!
//! If any of these change shape (return type, signature,
//! behaviour), this test fails. If a future builtin
//! simplifies its import (e.g. `use …::mint;` instead of
//! `use …::mint::CapabilityFactory;`), the test stays green
//! as long as the API still resolves — it asserts the
//! *behavioural* contract, not the import *shape*.
//!
//! Only `EchoBuiltin` is smoke-tested. The orchestrator
//! dispatches all three builtins (`echo` / `reverse` /
//! `database`) through the same `MintFn` shape (each
//! builtin's `register()` returns a non-capturing closure
//! of the same signature), so an echo round-trip exercises
//! the registry path; a per-builtin mint path is a thin
//! wrapper around `factory.mint(...)` and would fail
//! `cargo build` if it drifted.

use std::sync::Arc;
use std::time::Duration;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::CapabilitySpace;
use odyssey::capability::handle::slot::Slot;
use odyssey::core::clock::clock::SystemClock;
use odyssey::core::contract::builtin::BuiltinManifest;
use odyssey::core::identity::ids::PluginId;
use odyssey::core::identity::kind::CapKind;
use odyssey::core::meta::chunk::CapabilityChunk;
use odyssey::personality::lifecycle::mint::CapabilityFactory;
use odyssey_builtins::echo::{EchoBuiltin, EchoResource};
use odyssey_builtins::streaming_echo::{StreamingEchoBuiltin, StreamingEchoResource};
use tokio_stream::StreamExt;

#[test]
fn echo_builtin_round_trips_through_typed_mint() {
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    let builtin = EchoBuiltin;
    let manifest = builtin.manifest();
    let decl = &manifest.exposes[0];

    // Phase 10: call the inherent `mint` method directly.
    // The trait object dispatch (`&dyn Mint`) was deleted
    // along with the `Mint` trait; the registry holds a
    // non-capturing closure of the same shape, so calling
    // the inherent method exercises the exact code path
    // the orchestrator's `MintFn` does.
    let slot_id = builtin.mint(
        &factory,
        &PluginId {
            name: "echo".into(),
            version: "0.1.0".into(),
        },
        decl,
        CapKind::Sync,
        CapabilityBudget::new(5000),
        &[],
    );

    let slot: Slot<EchoResource> = Slot::new(cspace, slot_id);
    let input = serde_json::json!({"hello": "world"});
    let output = slot.invoke(input.clone()).expect("invoke should succeed");

    assert_eq!(
        output, input,
        "echo builtin must return its input unchanged"
    );
}

/// Phase 11 streaming smoke test. Exercises the full
/// `Resource::open` path: mint a streaming cap, call
/// `Slot::open(...)` to get a `Receiver<CapabilityChunk>`,
/// collect the chunks, assert shape + count. Bounded by
/// `tokio::time::timeout` so a hung producer fails the test
/// rather than hanging CI.
#[tokio::test(flavor = "current_thread")]
async fn streaming_echo_builtin_round_trips_through_typed_open() {
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    let builtin = StreamingEchoBuiltin;
    let manifest = builtin.manifest();
    let decl = &manifest.exposes[0];

    let slot_id = builtin.mint(
        &factory,
        &PluginId {
            name: "streaming_echo".into(),
            version: "0.1.0".into(),
        },
        decl,
        CapKind::Stream,
        CapabilityBudget::new(5000),
        &[],
    );

    let slot: Slot<StreamingEchoResource> = Slot::new(cspace, slot_id);
    let rx = slot
        .open(serde_json::json!({"text": "hi", "count": 3}))
        .expect("open should succeed");

    // Collect all chunks within a 2-second budget. A hung
    // producer fails the test instead of hanging the runner.
    let chunks: Vec<CapabilityChunk> = tokio::time::timeout(
        Duration::from_secs(2),
        tokio_stream::wrappers::ReceiverStream::new(rx).collect::<Vec<_>>(),
    )
    .await
    .expect("streaming_echo producer hung within 2s budget");

    assert_eq!(
        chunks.len(),
        4,
        "expected 3 items + 1 done = 4 chunks, got {}",
        chunks.len()
    );

    // Last chunk must be Done.
    assert!(
        matches!(chunks.last(), Some(CapabilityChunk::Done)),
        "last chunk should be Done, got {:?}",
        chunks.last()
    );

    // The 3 item chunks carry the right text and indices in
    // order.
    for (i, chunk) in chunks.iter().take(3).enumerate() {
        let CapabilityChunk::Item(value) = chunk else {
            panic!("expected Item at index {i}, got {chunk:?}");
        };
        assert_eq!(
            value.get("text").and_then(|v| v.as_str()),
            Some("hi"),
            "chunk {i} text mismatch"
        );
        assert_eq!(
            value.get("index").and_then(|v| v.as_u64()),
            Some(i as u64),
            "chunk {i} index mismatch"
        );
    }
}

/// Phase 11 typed-mismatch smoke test. Calling `Slot::invoke`
/// on a streaming cap must return `CapabilityError::KindMismatch`
/// (sync call shape on a streaming resource). The check is
/// sync because `Capability::invoke` is sync — the typed path
/// runs before any task spawn.
#[test]
fn streaming_echo_kind_mismatch_on_invoke() {
    use odyssey::capability::error::CapabilityError;

    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));

    let builtin = StreamingEchoBuiltin;
    let manifest = builtin.manifest();
    let decl = &manifest.exposes[0];

    let slot_id = builtin.mint(
        &factory,
        &PluginId {
            name: "streaming_echo".into(),
            version: "0.1.0".into(),
        },
        decl,
        CapKind::Stream,
        CapabilityBudget::new(5000),
        &[],
    );

    let slot: Slot<StreamingEchoResource> = Slot::new(cspace, slot_id);
    let err = slot
        .invoke(serde_json::json!({"text": "hi", "count": 3}))
        .expect_err("invoke on streaming cap must fail");
    match err {
        CapabilityError::KindMismatch { expected, got, .. } => {
            assert_eq!(expected, "sync");
            assert_eq!(got, "stream");
        }
        other => panic!("expected KindMismatch, got {other:?}"),
    }
}

/// The agent builtin's manifests are the only `requires` in the
/// workspace, and therefore the only reason the resolver emits a
/// binding row. This checks the manifest half and the resolver
/// half separately: the first assertion is the input contract
/// (what the manifest declares), the second is the output
/// contract (what the resolver builds from it).
///
/// The four provider plugins must be in the manifest list or the
/// resolver fails with `Unprovided` — which is what makes this a
/// real end-to-end check rather than a self-consistency check.
#[test]
fn agent_manifest_requires_resolve_to_a_binding_row_for_every_handle() {
    use odyssey::personality::composition::resolve::resolve;
    use odyssey_builtins::{agent, database, echo, reverse, streaming_echo};

    let manifests = vec![
        echo::EchoBuiltin.manifest(),
        reverse::ReverseBuiltin.manifest(),
        database::DatabaseBuiltin.manifest(),
        streaming_echo::StreamingEchoBuiltin.manifest(),
        agent::AgentListBuiltin.manifest(),
        agent::AgentDescribeBuiltin.manifest(),
    ];

    for manifest in &manifests {
        let is_agent = manifest.plugin.name.starts_with("agent_");
        if is_agent {
            assert_eq!(
                manifest.requires.len(),
                agent::REACHES.len(),
                "{} should require every handle it advertises",
                manifest.plugin.name
            );
        } else {
            assert!(
                manifest.requires.is_empty(),
                "{} is a provider and should declare no dependencies",
                manifest.plugin.name
            );
        }
    }

    let plan = resolve(&manifests).expect("the agent's requires must all be provided");

    // Every non-agent plugin publishes a contract and consumes
    // none, so its binding row is absent entirely.
    for manifest in manifests
        .iter()
        .filter(|m| !m.plugin.name.starts_with("agent_"))
    {
        assert!(
            !plan.bindings.contains_key(&manifest.plugin),
            "{} declares no requires, so it should have no binding row",
            manifest.plugin.name
        );
    }

    for manifest in manifests
        .iter()
        .filter(|m| m.plugin.name.starts_with("agent_"))
    {
        let rows = plan
            .bindings
            .get(&manifest.plugin)
            .unwrap_or_else(|| panic!("{} should have a binding row", manifest.plugin.name));
        assert_eq!(rows.len(), agent::REACHES.len());
        for (row, (handle, contract)) in rows.iter().zip(agent::REACHES) {
            assert_eq!(row.handle, handle);
            assert_eq!(row.contract, contract);
            assert_eq!(
                row.capability, contract,
                "each provider names its capability after its contract"
            );
        }
    }

    // The agent is minted after every provider it binds to, so
    // its handles are installed by the time its mint runs.
    let agent_at = plan
        .mint_order
        .iter()
        .position(|p| p.name == "agent_list")
        .expect("agent_list is in the mint order");
    for provider in ["echo", "reverse", "database", "streaming_echo"] {
        let provider_at = plan
            .mint_order
            .iter()
            .position(|p| p.name == provider)
            .expect("provider is in the mint order");
        assert!(
            provider_at < agent_at,
            "{provider} must mint before the agent that binds to it"
        );
    }
}

/// The agent's reachable set is its binding table, not the
/// cspace.
///
/// Three independent claims, each in its own cspace so that no
/// assertion depends on another's leftovers:
///
/// 1. A capability that is installed and live, but appears in no
///    binding row, is unreachable.
/// 2. A row whose capability was revoked resolves to
///    `live: false` and reports no rights — rather than
///    disappearing from the list or reporting stale metadata.
/// 3. A row with an empty capability name is an `Unbound` fault,
///    which fails the whole `list` instead of quietly dropping
///    one entry.
#[test]
fn agent_reaches_only_its_bindings_and_reports_revocation() {
    use odyssey::core::manifest::manifest::CapabilityDecl;
    use odyssey::personality::composition::resolve::ResolvedBinding;
    use odyssey_builtins::agent::{AgentCore, AgentError};

    let plugin = |name: &str| PluginId {
        name: name.into(),
        version: "0.1.0".into(),
    };
    let manifest = EchoBuiltin.manifest();

    let echo_row = |handle: &str| ResolvedBinding {
        handle: handle.into(),
        provider: plugin("echo"),
        capability: "echo".into(),
        contract: "echo".into(),
    };

    // 1. Live in the cspace, absent from the table.
    //
    // The registered name comes from `CapabilityDecl::name` (see
    // `meta_from_decl`), not from the plugin, so this declares a
    // capability named `plain` while the agent's table names
    // `echo`.
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));
    let plain_decl = CapabilityDecl {
        name: "plain".into(),
        in_type: "any".into(),
        out_type: "any".into(),
        kind: CapKind::Sync,
        contract_name: "plain".into(),
    };
    EchoBuiltin.mint(
        &factory,
        &plugin("plain"),
        &plain_decl,
        CapKind::Sync,
        CapabilityBudget::new(5000),
        &[],
    );
    let core = AgentCore::new(cspace.clone(), vec![echo_row("echo")]);
    assert!(
        cspace.lookup_by_name("plain").is_some(),
        "the capability is installed and reachable by name"
    );
    assert!(
        cspace.lookup_by_name("echo").is_none(),
        "nothing is registered under the name the table uses"
    );
    assert!(
        core.reach("echo").expect("known handle").live.is_none(),
        "so a row naming `echo` resolves to nothing"
    );
    assert_eq!(
        core.reach("extra").err(),
        Some(AgentError::Unknown("extra".into())),
        "a handle outside the table is unknown however many caps the cspace holds"
    );

    // 2. A row whose capability is revoked.
    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));
    let slot = EchoBuiltin.mint(
        &factory,
        &plugin("echo"),
        &manifest.exposes[0],
        CapKind::Sync,
        CapabilityBudget::new(5000),
        &[],
    );
    let core = AgentCore::new(cspace.clone(), vec![echo_row("echo")]);

    let described = core.describe("echo").expect("describe a live handle");
    assert_eq!(described["live"], serde_json::json!(true));
    assert_eq!(described["kind"], serde_json::json!("sync"));
    assert_eq!(described["operations"].as_array().map(Vec::len), Some(4));

    cspace.revoke_tree(slot);

    let described = core.describe("echo").expect("the row is still known");
    assert_eq!(described["live"], serde_json::json!(false));
    assert_eq!(
        described["capability"],
        serde_json::json!("echo"),
        "a gone capability still reports which name it would have had"
    );
    assert!(
        described.get("operations").is_none(),
        "a gone capability must not report rights it no longer holds"
    );
    let listed = core.list().expect("list resolves the dead row too");
    assert_eq!(listed["handles"][0]["live"], serde_json::json!(false));

    // 3. A row with no capability name.
    let faulted = AgentCore::new(
        cspace.clone(),
        vec![
            echo_row("ok"),
            ResolvedBinding {
                handle: "broken".into(),
                provider: plugin("echo"),
                capability: String::new(),
                contract: "echo".into(),
            },
        ],
    );
    assert_eq!(
        faulted.reach("broken").err(),
        Some(AgentError::Unbound("broken".into()))
    );
    assert_eq!(
        faulted.list().err(),
        Some(AgentError::Unbound("broken".into())),
        "one unbound row fails the whole list rather than being dropped"
    );
}
