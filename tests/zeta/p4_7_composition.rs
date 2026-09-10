//! ζ.31–ζ.33 — Phase 4 P4.7 Agent Composition tests.
//!
//! Supervisor holds (echo, database, sandbox) with full
//! authority. It restricts each cap to a slice and delegates
//! to a sub-agent:
//!
//! - **Research**  ← echo (READ-only)         — research lookup
//! - **Writer**    ← database (WRITE-only)    — write results
//! - **Coder**     ← sandbox (EXECUTE-only)   — run snippets
//!
//! Each sub-agent runs a program that targets its slice.
//! The proof: each sub-agent succeeds on its own slice and
//! is denied on slices it didn't receive. Same code path
//! (`AgentResource::open`); different reachable → different
//! outcomes.

use odyssey::host::resolver::Reachable;
use odyssey::kernel::{
    CapabilityChunk, CapabilityRights, OperationRights, Resource,
};
use odyssey::plugins::agent::{AgentResource, ProgramStep};
use odyssey::plugins::database::{handler as database_handler, DatabaseResource};
use odyssey::plugins::echo::basic::{handler as echo_handler, EchoResource};
use odyssey::plugins::sandbox::{handler as sandbox_handler, SandboxResource};
use serde_json::{json, Value};

// =========================================================================
// Shared fixture
// =========================================================================

struct World {
    space: odyssey::kernel::CapabilitySpace,
    echo_slot: odyssey::kernel::SlotId,
    database_slot: odyssey::kernel::SlotId,
    sandbox_slot: odyssey::kernel::SlotId,
}

fn build_world() -> World {
    use odyssey::kernel::space::CapabilitySpace;
    use odyssey::kernel::space::events::GraphEventBus;
    use odyssey::host::factory::CapabilityFactory;
    use odyssey::host::manifest::{CapabilityDecl, IsolationMode, PluginManifest};
    use odyssey::kernel::PluginId;

    let bus = GraphEventBus::default();
    let space = CapabilitySpace::with_bus(bus);
    let factory = CapabilityFactory::new(space.clone());

    let echo_decl = PluginManifest {
        plugin: PluginId { name: "echo".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "echo".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: "echo".into(),
            authority: odyssey::kernel::AuthorityContract::empty(),
            protocol: odyssey::kernel::Protocol::empty(),
        }],
        requires: vec![],
        consumes: vec![],
        host: vec![],
        resources: Default::default(),
    };
    let echo_slot = factory.mint::<EchoResource>(
        odyssey::kernel::CapKind::Sync,
        &echo_decl.exposes[0],
        &echo_decl.plugin,
        odyssey::kernel::CapabilityBudget::new(5000),
        echo_handler(),
    );

    let db_decl = PluginManifest {
        plugin: PluginId { name: "database".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "database".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: "database".into(),
            authority: odyssey::kernel::AuthorityContract::empty()
                .with_action("read", "DB_READ")
                .with_action("write", "DB_WRITE"),
            protocol: odyssey::kernel::Protocol::empty(),
        }],
        requires: vec![],
        consumes: vec![],
        host: vec![],
        resources: Default::default(),
    };
    let db_slot = factory.mint::<DatabaseResource>(
        odyssey::kernel::CapKind::Sync,
        &db_decl.exposes[0],
        &db_decl.plugin,
        odyssey::kernel::CapabilityBudget::new(5000),
        database_handler(),
    );

    // Sandbox: only EXECUTE authority.
    let sb_decl = PluginManifest {
        plugin: PluginId { name: "sandbox".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "exec".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: "sandbox".into(),
            authority: odyssey::kernel::AuthorityContract::empty()
                .with_action("execute", "EXECUTE"),
            protocol: odyssey::kernel::Protocol::empty(),
        }],
        requires: vec![],
        consumes: vec![],
        host: vec![],
        resources: Default::default(),
    };
    let sb_slot = factory.mint::<SandboxResource>(
        odyssey::kernel::CapKind::Sync,
        &sb_decl.exposes[0],
        &sb_decl.plugin,
        odyssey::kernel::CapabilityBudget::new(5000),
        sandbox_handler(),
    );

    World { space, echo_slot, database_slot: db_slot, sandbox_slot: sb_slot }
}

async fn drain_events(mut rx: tokio::sync::mpsc::Receiver<CapabilityChunk>) -> Vec<Value> {
    let mut events: Vec<Value> = Vec::new();
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
            Ok(Some(CapabilityChunk::Item(v))) => events.push(v),
            Ok(Some(CapabilityChunk::Done)) => break,
            Ok(None) => break,
            Err(_) => panic!("agent stream timed out"),
        }
    }
    events
}

fn build_agent(
    name: &str,
    reachable: Vec<Reachable>,
    space: odyssey::kernel::CapabilitySpace,
) -> std::sync::Arc<AgentResource> {
    std::sync::Arc::new(AgentResource::from_reachable(name, reachable, space))
}

// =========================================================================
// ζ.31 — Three sub-agents, each with a distinct slice
// =========================================================================

#[tokio::test]
async fn supervisor_delegates_three_distinct_slices_to_three_sub_agents() {
    let world = build_world();

    // Supervisor restricts each cap to a slice for one sub-agent.
    // (Phase 4 doesn't yet have a runtime `delegate()` API;
    // the act of restricting IS the delegation. Supervisor
    // would normally do this in its own program, but for the
    // test we restrict at setup time.)

    // Research ← echo (READ-only — echo has no published actions,
    // but the slot is held; cap.invoke_op_dyn still uses
    // EXECUTE so it succeeds). For the test, restricting to
    // a single EXECUTE bit is enough to exercise the slice.
    let research_slice = world.space.restrict::<EchoResource>(
        world.echo_slot,
        CapabilityRights {
            operations: OperationRights::EXECUTE,
            timeout_ms: 5000,
        },
        "echo_research".into(),
    ).expect("restrict echo for research");

    // Writer ← database (WRITE-only).
    let writer_slice = world.space.restrict::<DatabaseResource>(
        world.database_slot,
        CapabilityRights {
            operations: OperationRights::WRITE,
            timeout_ms: 5000,
        },
        "db_writer".into(),
    ).expect("restrict db for writer");

    // Coder ← sandbox (EXECUTE-only).
    let coder_slice = world.space.restrict::<SandboxResource>(
        world.sandbox_slot,
        CapabilityRights {
            operations: OperationRights::EXECUTE,
            timeout_ms: 5000,
        },
        "sb_coder".into(),
    ).expect("restrict sb for coder");

    // The three slices are distinct slot ids.
    assert_ne!(research_slice.raw(), writer_slice.raw());
    assert_ne!(writer_slice.raw(), coder_slice.raw());
    assert_ne!(research_slice.raw(), coder_slice.raw());

    // Build three sub-agents, each with reachable pointing at
    // ITS slice.
    let research = build_agent(
        "research",
        vec![Reachable::new("lookup", "echo_research")],
        world.space.clone(),
    );
    let writer = build_agent(
        "writer",
        vec![Reachable::new("store", "db_writer")],
        world.space.clone(),
    );
    let coder = build_agent(
        "coder",
        vec![Reachable::new("exec", "sb_coder")],
        world.space.clone(),
    );

    // Each runs a program targeting its slice.
    let research_events = drain_events(
        research.open(json!({
            "program": [
                { "handle": "lookup", "input": "research query" },
            ]
        })).unwrap(),
    ).await;
    let writer_events = drain_events(
        writer.open(json!({
            "program": [
                { "handle": "store", "op": "write",
                  "input": { "op": "write", "key": "k", "value": "v" } },
            ]
        })).unwrap(),
    ).await;
    let coder_events = drain_events(
        coder.open(json!({
            "program": [
                { "handle": "exec", "op": "execute", "input": { "path": "src/plugins/sandbox/sandbox_programs/hello.wat" } },
            ]
        })).unwrap(),
    ).await;

    // All three should have 1 step_ok, 0 deny, 0 fail, 0 skip.
    for (label, events) in [
        ("research", &research_events),
        ("writer", &writer_events),
        ("coder", &coder_events),
    ] {
        let done = events.last().expect("done present");
        assert_eq!(done["ok"], 1, "{label} should step_ok; events: {events:?}");
        assert_eq!(done["denied"], 0, "{label} should not deny");
        assert_eq!(done["failed"], 0, "{label} should not fail");
        assert_eq!(done["skipped"], 0, "{label} should not skip");
    }
}

// =========================================================================
// ζ.32 — Sub-agent denied when its slice lacks the required op
// =========================================================================
//
// P1-b (Phase 4 review loop): the original `sub_agent_denied_on_caps_outside_its_slice`
// exercised reachability-skip (`step_skip` because the handle wasn't in
// the agent's reachable), not authority-deny (`step_deny` because the
// reachable held a cap that didn't hold the required op bit). Those
// are different paths in run_program: reachability-skip is the P4.5
// "env doesn't grant this capability" path; authority-deny is the
// P4.6 "kernel-level guard refused the requested op" path. This
// rewrite gives research a reachable handle pointing at a restricted
// (READ-only) database slice, then asks it to `write`. The handle is
// in reachable, the cap is in cspace, but the kernel-level
// `invoke_op_dyn` guard refuses WRITE — `step_deny`. That's the
// authority axis, properly tested.

#[tokio::test]
async fn sub_agent_denied_on_op_outside_its_slice() {
    let world = build_world();

    // Researcher receives a READ-only database slice. The
    // reachable references the derived slice by its cspace
    // name, so handle → capability resolution succeeds; the
    // authority check then refuses the requested WRITE.
    let research_db_slice = world.space.restrict::<DatabaseResource>(
        world.database_slot,
        CapabilityRights {
            operations: OperationRights::READ,
            timeout_ms: 5000,
        },
        "db_research".into(),
    ).expect("restrict db for research");

    // Research's reachable has "store" → db_research (the
    // READ-only slice). Note: handle is "store" (what the
    // research program would naturally call it), but the
    // capability backing it is the restricted slice, not
    // the full-rights database cap.
    let research = build_agent(
        "research",
        vec![Reachable::new("store", "db_research")],
        world.space.clone(),
    );

    // Research tries to write via "store". The handle is
    // reachable; the cap is in cspace; the cap's held
    // ops include READ but not WRITE; the kernel-level
    // invoke_op_dyn guard refuses. Step emits `step_deny`.
    let events = drain_events(
        research.open(json!({
            "program": [
                { "handle": "store", "op": "write",
                  "input": { "op": "write", "key": "k", "value": "v" } },
            ]
        })).unwrap(),
    ).await;

    let done = events.last().unwrap();
    assert_eq!(
        done["denied"], 1,
        "write against READ-only slice must step_deny; events: {events:?}"
    );
    assert_eq!(done["ok"], 0);
    assert_eq!(done["skipped"], 0);
    assert_eq!(done["failed"], 0);

    // The deny event must surface the kernel's operation
    // refusal, not the authority-table miss.
    let deny = events
        .iter()
        .find(|e| e["event"] == "step_deny")
        .expect("step_deny present");
    let reason = deny["reason"].as_str().unwrap();
    assert!(
        reason.contains("operation denied") || reason.contains("not in held"),
        "step_deny reason must reflect authority refusal; got: {reason}"
    );

    // Sanity: the same research agent, same reachable, can
    // still read via the same handle (the slice's READ bit
    // is held). This proves the deny above is specifically
    // about the op axis, not the reachable axis.
    let read_events = drain_events(
        research.open(json!({
            "program": [
                { "handle": "store", "op": "read",
                  "input": { "op": "read", "key": "k" } },
            ]
        })).unwrap(),
    ).await;
    let read_done = read_events.last().unwrap();
    assert_eq!(
        read_done["ok"], 1,
        "read against READ-only slice must step_ok; events: {read_events:?}"
    );

    let _ = research_db_slice;
}

// =========================================================================
// ζ.33 — All three sub-agents running concurrently is the agent graph
// =========================================================================

#[tokio::test]
async fn three_sub_agents_running_concurrently_form_a_capability_graph() {
    let world = build_world();

    // Supervisor's full set: echo + database + sandbox.
    // Three delegated slices:
    let _echo_r = world.space.restrict::<EchoResource>(
        world.echo_slot,
        CapabilityRights { operations: OperationRights::EXECUTE, timeout_ms: 5000 },
        "echo_research".into(),
    ).unwrap();
    let _db_w = world.space.restrict::<DatabaseResource>(
        world.database_slot,
        CapabilityRights { operations: OperationRights::WRITE, timeout_ms: 5000 },
        "db_writer".into(),
    ).unwrap();
    let _sb_c = world.space.restrict::<SandboxResource>(
        world.sandbox_slot,
        CapabilityRights { operations: OperationRights::EXECUTE, timeout_ms: 5000 },
        "sb_coder".into(),
    ).unwrap();

    let research = build_agent(
        "research",
        vec![Reachable::new("lookup", "echo_research")],
        world.space.clone(),
    );
    let writer = build_agent(
        "writer",
        vec![Reachable::new("store", "db_writer")],
        world.space.clone(),
    );
    let coder = build_agent(
        "coder",
        vec![Reachable::new("exec", "sb_coder")],
        world.space.clone(),
    );

    // Run all three programs concurrently. They share the
    // cspace but each only sees its own slice.
    let (r_events, w_events, c_events) = tokio::join!(
        drain_events(research.open(json!({"program":[{"handle":"lookup","input":"q"}]})).unwrap()),
        drain_events(writer.open(json!({"program":[{"handle":"store","op":"write","input":{"op":"write","key":"k","value":"v"}}]})).unwrap()),
        drain_events(coder.open(json!({"program":[{"handle":"exec","op":"execute","input":{"path":"src/plugins/sandbox/sandbox_programs/hello.wat"}}]})).unwrap()),
    );

    let r_done = r_events.last().unwrap();
    let w_done = w_events.last().unwrap();
    let c_done = c_events.last().unwrap();

    assert_eq!(r_done["ok"], 1, "research ok");
    assert_eq!(w_done["ok"], 1, "writer ok");
    assert_eq!(c_done["ok"], 1, "coder ok");

    // The capability graph: 3 derived slots, each pointed at
    // by exactly one sub-agent's reachable. Supervisor's
    // original 3 caps are still in the cspace (never revoked).
    // The "graph" is: parent × 3 → child × 3 derived.

    // Phase 4 P4.7 proof: this is the multi-agent graph.
    // Each agent is its own program interpreter. The
    // cspace is the shared resource. Each agent's reachable
    // is its edge into the graph. There is no "agent
    // hierarchy" — there are only capabilities.
    let _ = std::marker::PhantomData::<ProgramStep>;
}

// =========================================================================
// Edge test 4 — Child agent observes parent revoke_tree (cascade)
// =========================================================================

#[tokio::test]
async fn child_agent_observable_after_parent_revoke_tree() {
    use odyssey::kernel::{CapabilityRights, OperationRights};

    let world = build_world();

    // Mint a parent slot from echo and a derived child slot
    // (the delegation token the child agent will see).
    let parent = world.echo_slot;
    let child = world
        .space
        .restrict::<EchoResource>(
            parent,
            CapabilityRights {
                operations: OperationRights::EXECUTE,
                timeout_ms: 5000,
            },
            "echo_child".into(),
        )
        .expect("restrict parent for child");

    // Build a child agent whose reachable points at the
    // derived (child) slot.
    let child_agent = build_agent(
        "child",
        vec![Reachable::new("echo_in_child", "echo_child")],
        world.space.clone(),
    );

    // Open the child agent's program on a sibling task.
    let mut rx = child_agent
        .open(json!({
            "program": [
                { "handle": "echo_in_child", "input": "first" },
                { "handle": "echo_in_child", "input": "second" },
            ]
        }))
        .expect("child_agent.open");

    // Wait until the first step has emitted step_start.
    let _pre_revoke = drain_until_step_starts_count(&mut rx, 1).await;

    // Cascade: revoke the parent slot tree. The child slot
    // must be torn down with it.
    let removed = world.space.revoke_tree(parent);
    assert!(
        removed >= 2,
        "revoke_tree must remove parent + child; removed={removed}"
    );

    // Drain the rest. Subsequent steps targeting echo_in_child
    // must step_skip ("reachable but cap missing in cspace")
    // because the child slot is now empty.
    let after_revoke = drain_events(rx).await;
    let mut all = _pre_revoke;
    all.extend(after_revoke);
    let done = all.last().expect("done event present");

    // Step 1 may or may not have completed before the
    // cascade — the timing is cooperative. The invariant:
    // every step that ran after the cascade sees a missing
    // cap. Failed == 0 (no panics); denied == 0 (this is
    // a missing-in-cspace path, not an authority path).
    assert_eq!(done["failed"], 0, "no panics from cascade; events: {all:?}");
    assert_eq!(done["denied"], 0, "cascade is missing-in-cspace, not authority; events: {all:?}");

    // Every step_skip event's reason must surface "missing
    // in cspace".
    for skip in all.iter().filter(|e| e["event"] == "step_skip") {
        let reason = skip["reason"].as_str().unwrap();
        assert!(
            reason.contains("missing in cspace"),
            "cascade must produce missing-in-cspace skips; got: {reason}"
        );
    }

    let _ = child;
}

/// Drain helper local to edge test 4: count-based step_start
/// drain (returns once `count` step_starts have been seen).
async fn drain_until_step_starts_count(
    rx: &mut tokio::sync::mpsc::Receiver<odyssey::kernel::CapabilityChunk>,
    count: usize,
) -> Vec<Value> {
    let mut events: Vec<Value> = Vec::new();
    let mut seen = 0usize;
    while seen < count {
        match tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await {
            Ok(Some(odyssey::kernel::CapabilityChunk::Item(v))) => {
                if v["event"] == "step_start" {
                    seen += 1;
                }
                events.push(v);
            }
            Ok(Some(odyssey::kernel::CapabilityChunk::Done)) => break,
            Ok(None) => break,
            Err(_) => panic!("timed out waiting for step_starts; saw {events:?}"),
        }
    }
    events
}
