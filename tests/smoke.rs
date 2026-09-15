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

/// A plugin that requires its own contract is refused as
/// `SelfRequirement`, not as a `Cycle`.
///
/// The distinction matters to whoever reads the boot failure: a cycle
/// between plugins is a graph mistake, whereas this one has no mint
/// order at all. It also pins that the answer does not depend on the
/// topological sort noticing the self-edge second-hand through
/// in-degree bookkeeping.
#[test]
fn plugin_requiring_its_own_contract_is_a_self_requirement() {
    use odyssey::core::manifest::manifest::ManifestBuilder;
    use odyssey::personality::composition::resolve::{ResolveError, resolve};

    let self_requiring = ManifestBuilder::new("ouroboros")
        .expose("loop", "loop")
        .requires("loop", "loop")
        .build();

    match resolve(&[self_requiring]) {
        Err(ResolveError::SelfRequirement { plugin, contract }) => {
            assert_eq!(plugin.name, "ouroboros");
            assert_eq!(contract, "loop");
        }
        other => panic!("expected SelfRequirement, got {other:?}"),
    }

    // A mutual pair is still a genuine cycle, not a self-requirement.
    let a = ManifestBuilder::new("a")
        .expose("a_cap", "a_contract")
        .requires("b_cap", "b_contract")
        .build();
    let b = ManifestBuilder::new("b")
        .expose("b_cap", "b_contract")
        .requires("a_cap", "a_contract")
        .build();
    match resolve(&[a, b]) {
        Err(ResolveError::Cycle { chain }) => {
            assert_eq!(chain.len(), 2, "both plugins form the cycle: {chain:?}");
        }
        other => panic!("expected Cycle for a mutual pair, got {other:?}"),
    }
}

/// `agent_describe`'s input shape is closed.
///
/// The agent observes and never invokes, so a body carrying an extra
/// field — the obvious guess being `op` — must be refused rather than
/// answered. Accepting it would let a caller read a description as
/// evidence that a dispatch happened.
#[test]
fn agent_describe_refuses_unknown_fields() {
    use odyssey::core::contract::resource::Resource;
    use odyssey::core::manifest::manifest::CapabilityDecl;
    use odyssey_builtins::agent::AgentDescribeResource;
    use odyssey_builtins::echo::EchoBuiltin;

    let cspace = CapabilitySpace::new();
    let factory = CapabilityFactory::with_clock(cspace.clone(), Arc::new(SystemClock));
    let decl = CapabilityDecl {
        name: "echo".into(),
        in_type: "any".into(),
        out_type: "any".into(),
        kind: CapKind::Sync,
        contract_name: "echo".into(),
    };
    EchoBuiltin.mint(
        &factory,
        &PluginId {
            name: "echo".into(),
            version: "0.1.0".into(),
        },
        &decl,
        CapKind::Sync,
        CapabilityBudget::new(5000),
        &[],
    );

    let resource = AgentDescribeResource::new(
        cspace,
        vec![
            odyssey::personality::composition::resolve::ResolvedBinding {
                handle: "echo".into(),
                provider: PluginId {
                    name: "echo".into(),
                    version: "0.1.0".into(),
                },
                capability: "echo".into(),
                contract: "echo".into(),
            },
        ],
    );

    // The shape the operation documents still works.
    let ok = resource
        .invoke(serde_json::json!({"handle": "echo"}))
        .expect("the documented shape must still be accepted");
    assert_eq!(ok["live"], serde_json::json!(true));

    // An extra field is refused, and the message names it.
    let err = resource
        .invoke(serde_json::json!({"handle": "echo", "op": "invoke"}))
        .expect_err("an unknown field must be refused");
    assert!(err.contains("unknown field `op`"), "unhelpful error: {err}");
    assert!(
        err.contains("handle"),
        "the error should name the accepted field: {err}"
    );

    // And the refusal happens before any capability is resolved, so a
    // bad field never reaches the cspace.
    let err = resource
        .invoke(serde_json::json!({"handle": "nope", "op": "invoke"}))
        .expect_err("the field check precedes the handle lookup");
    assert!(
        err.contains("unknown field"),
        "expected the field error first, got: {err}"
    );
}

/// The frontend's inline script reads the keys the live API returns.
///
/// This is the only test that exercises the HTTP bridge and the page
/// together. Both halves were already covered separately — `curl`
/// against the endpoints, and reading the HTML — and neither catches the
/// failure that matters here: a field renamed on one side while the
/// other keeps looking for the old name, which renders as a dash rather
/// than an error. `tests/frontend.mjs` loads the real page script into a
/// DOM shim, drives it against a server this test spawns, and asserts
/// the resulting element tree.
///
/// Skipped when `node` is unavailable: the assertion needs a JS engine,
/// and silently passing would be worse than not running.
#[test]
fn frontend_script_binds_to_the_live_api() {
    use std::io::Write;
    use std::net::{SocketAddr, TcpStream};
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::thread::sleep;

    // Kills the spawned server on every exit path, panic included, so a
    // failed assertion cannot leave port 3030 held.
    struct Server(std::process::Child);

    impl Server {
        fn stop(&mut self) {
            let pid = self.0.id() as i32;
            // SAFETY: `kill` takes an integer pid and cannot invalidate
            // memory; the child is still live here.
            unsafe {
                libc::kill(pid, libc::SIGINT);
            }
            for _ in 0..20 {
                if matches!(self.0.try_wait(), Ok(Some(_))) {
                    return;
                }
                sleep(Duration::from_millis(50));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    impl Drop for Server {
        fn drop(&mut self) {
            self.stop();
        }
    }

    let Ok(node) = Command::new("node").arg("--version").output() else {
        eprintln!("skipping: node is not installed");
        return;
    };
    assert!(
        node.status.success(),
        "node is installed but unusable: {}",
        String::from_utf8_lossy(&node.stderr)
    );

    let bin: PathBuf = match option_env!("CARGO_BIN_EXE_basic") {
        Some(path) => path.into(),
        // Cargo sets CARGO_BIN_EXE_* for [[bin]] targets; `basic` is an
        // [[example]], so fall back to the default target layout this
        // test itself was built into.
        None => {
            let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            dir.push("target");
            dir.push(std::env::var("PROFILE").unwrap_or_else(|_| "debug".into()));
            dir.push("examples");
            dir.push("basic");
            dir
        }
    };
    assert!(
        bin.exists(),
        "the example binary is missing at {} — run `cargo build --example basic` first",
        bin.display()
    );

    // Ask the OS for a free port and hand it to the child. A fixed port
    // would mean competing with whatever already holds it — and, worse, a
    // connect would still succeed against that other server, so every
    // assertion below would silently judge a build that is not the one
    // under test. Measured: a stale server on the fixed port made an
    // earlier version of this test pass while reading the old binary.
    let addr: SocketAddr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").expect("a free port");
        let addr = probe.local_addr().expect("the probe's address");
        drop(probe);
        addr
    };

    let mut server = Server(
        Command::new(&bin)
            .env("ODYSSEY_ADDR", addr.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the example binary should spawn"),
    );

    // Poll the bridge rather than sleeping a fixed amount: the server
    // binds after minting, so a fixed wait is either slow or flaky.
    let mut up = false;
    for _ in 0..100 {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
            up = true;
            break;
        }
        if server.0.try_wait().ok().flatten().is_some() {
            break;
        }
        sleep(Duration::from_millis(100));
    }

    // Even on a private port, a connect can succeed against a stranger if
    // the child died before binding and something else took the port in
    // between. Reaping the child is the only way to tell "bound it" from
    // "lost the race" — and the settle delay is load-bearing, because a
    // `try_wait` taken the instant a connect succeeds can still see a
    // process that is on its way out.
    sleep(Duration::from_millis(500));
    if let Ok(Some(status)) = server.0.try_wait() {
        panic!(
            "the example binary exited with {status} instead of serving {addr}; \
             the assertions would have run against whatever answers that port"
        );
    }
    assert!(up, "the HTTP bridge never came up on {addr}");

    let page: PathBuf = [env!("CARGO_MANIFEST_DIR"), "examples/frontend/index.html"]
        .iter()
        .collect();

    let mut script = std::env::temp_dir();
    script.push(format!("odyssey-frontend-{}.mjs", std::process::id()));
    std::fs::File::create(&script)
        .and_then(|mut f| f.write_all(include_bytes!("frontend.mjs")))
        .expect("the frontend test script should be writable");

    let out = Command::new("node")
        .arg(&script)
        .env("ODYSSEY_URL", format!("http://{addr}"))
        .env("ODYSSEY_PAGE", &page)
        .output()
        .expect("node should run the frontend test script");
    let _ = std::fs::remove_file(&script);

    assert!(
        out.status.success(),
        "frontend data-binding check failed:\n{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("frontend data-binding OK"),
        "the check exited 0 without reporting success: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}
