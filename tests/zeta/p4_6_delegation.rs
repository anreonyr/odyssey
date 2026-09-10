//! ζ.28–ζ.30 — Phase 4 P4.6 Capability Delegation tests.
//!
//! ## P4.6 — Supervisor restricts, Researcher receives slice
//!
//! The Phase 4 thesis property: `behavior = program +
//! capability environment`. P4.5 proved the case where
//! environments differ by *presence* (a capability either
//! reachable or not). P4.6 proves the case where environments
//! differ by *authority* (the same capability, different
//! bits held).
//!
//! - **ζ.28**: Supervisor holds database with READ+WRITE.
//!   Supervisor's program step `database write "k" "v"`
//!   succeeds. After Supervisor calls `cspace.restrict(...)`
//!   to derive a `database_readonly` slot, Researcher built
//!   with that restricted slot in its reachable runs the
//!   same program step → `step_deny` (WRITE not in held ops).
//!   Same program. Different held authority. Different
//!   outcomes.
//!
//! - **ζ.29**: Attenuation violation: trying to derive a
//!   slot that *exceeds* the source's authority is rejected
//!   (`AttenuationViolation`). This is the "no amplification"
//!   property — you can't grant what you don't have.
//!
//! - **ζ.30**: The same restricted cap can be granted to
//!   multiple consumers; each consumer sees the same
//!   attenuated authority independently. The cap graph
//!   remains a DAG (Supervisor → {Researcher, Writer} all
//!   receive slices of the same parent).

use odyssey::capability::{CapabilityChunk, CapabilityRights, OperationRights, Reachable, Resource};
use odyssey::plugins::agent::{AgentResource, ProgramStep};
use odyssey::plugins::database::{handler as database_handler, DatabaseResource};
use odyssey::plugins::echo::basic::{handler as echo_handler, EchoResource};
use serde_json::{json, Value};

// =========================================================================
// Shared fixture — cspace with echo + database, plus helpers.
// =========================================================================

struct World {
    space: odyssey::capability::CapabilitySpace,
    echo_slot: odyssey::capability::SlotId,
    database_slot: odyssey::capability::SlotId,
}

fn build_world() -> World {
    use odyssey::capability::cspace::CapabilitySpace;
    use odyssey::capability::events::GraphEventBus;
    use odyssey::kernel::factory::CapabilityFactory;
    use odyssey::kernel::manifest::{
        CapabilityDecl, IsolationMode, PluginId, PluginManifest,
    };

    let bus = GraphEventBus::default();
    let space = CapabilitySpace::with_bus(bus);
    let factory = CapabilityFactory::new(space.clone());

    // Mint echo with full rights.
    let echo_decl = PluginManifest {
        plugin: PluginId { name: "echo".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "echo".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: "echo".into(),
            authority: odyssey::capability::AuthorityContract::empty(),
            protocol: odyssey::capability::Protocol::empty(),
        }],
        requires: vec![],
        consumes: vec![],
        host: vec![],
        resources: Default::default(),
    };
    let echo_slot = factory.mint::<EchoResource>(
        odyssey::capability::CapKind::Sync,
        &echo_decl.exposes[0],
        &echo_decl.plugin,
        odyssey::capability::CapabilityBudget::new(5000),
        echo_handler(),
    );

    // Mint database with full authority (READ+WRITE+ADMIN).
    let db_decl = PluginManifest {
        plugin: PluginId { name: "database".into(), version: "0.1.0".into() },
        isolate: IsolationMode::InProc,
        exposes: vec![CapabilityDecl {
            name: "database".into(),
            in_type: "any".into(),
            out_type: "any".into(),
            streaming: false,
            contract_name: "database".into(),
            authority: odyssey::capability::AuthorityContract::empty()
                .with_action("read", "DB_READ")
                .with_action("write", "DB_WRITE")
                .with_action("admin", "DB_ADMIN"),
            protocol: odyssey::capability::Protocol::empty(),
        }],
        requires: vec![],
        consumes: vec![],
        host: vec![],
        resources: Default::default(),
    };
    let db_slot = factory.mint::<DatabaseResource>(
        odyssey::capability::CapKind::Sync,
        &db_decl.exposes[0],
        &db_decl.plugin,
        odyssey::capability::CapabilityBudget::new(5000),
        database_handler(),
    );

    World { space, echo_slot, database_slot: db_slot }
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

/// Build an AgentResource with a hand-rolled reachable set.
/// Used in delegation tests where the reachable references
/// derived slots (not visible to the resolver).
fn agent_with_reachable(
    name: &str,
    reachable: Vec<Reachable>,
    cspace: odyssey::capability::CapabilitySpace,
) -> std::sync::Arc<AgentResource> {
    std::sync::Arc::new(AgentResource::from_reachable(name, reachable, cspace))
}

// =========================================================================
// ζ.28 — Supervisor→Researcher: same program, different held authority
// =========================================================================

#[tokio::test]
async fn supervisor_delegates_restricted_db_to_researcher() {
    let world = build_world();

    // Same program for both. Both agents have "database" in
    // their reachable, but Supervisor's slot holds
    // READ+WRITE+ADMIN while Researcher's slot holds only READ.
    let program = json!({
        "program": [
            { "handle": "database", "op": "write",
              "input": { "op": "write", "key": "k", "value": "v" } },
        ]
    });

    // --- Supervisor: full database authority.
    let supervisor_reachable = vec![Reachable::new("database", "database")];
    let supervisor_agent = agent_with_reachable(
        "supervisor",
        supervisor_reachable,
        world.space.clone(),
    );

    // Run program on supervisor.
    let events_supervisor =
        drain_events(supervisor_agent.open(program.clone()).unwrap()).await;
    let done_supervisor = events_supervisor.last().unwrap().clone();

    // --- Supervisor delegates by restricting database to READ-only.
    let _researcher_slot = world.space.restrict::<DatabaseResource>(
        world.database_slot,
        CapabilityRights { operations: OperationRights::READ, timeout_ms: 5000 },
        "database_researcher".into(),
    ).expect("restrict ok");

    // --- Researcher: READ-only database.
    let researcher_reachable =
        vec![Reachable::new("database", "database_researcher")];
    let researcher_agent = agent_with_reachable(
        "researcher",
        researcher_reachable,
        world.space.clone(),
    );

    // Run the SAME program on researcher.
    let events_researcher =
        drain_events(researcher_agent.open(program.clone()).unwrap()).await;
    let done_researcher = events_researcher.last().unwrap().clone();

    // --- Compare outcomes.
    // Supervisor: write succeeds (full authority).
    assert_eq!(
        done_supervisor["ok"], 1,
        "supervisor should have step_ok; events: {events_supervisor:?}"
    );
    assert_eq!(done_supervisor["denied"], 0);
    assert_eq!(done_supervisor["failed"], 0);

    // Researcher: write denied (READ-only authority).
    assert_eq!(
        done_researcher["denied"], 1,
        "researcher should have step_deny; events: {events_researcher:?}"
    );
    assert_eq!(done_researcher["ok"], 0);
    assert_eq!(done_researcher["failed"], 0);

    // The Phase 4 P4.6 proof:
    //   Same program. Different held authority. Different outcomes.
    assert_ne!(done_supervisor["ok"], done_researcher["ok"]);
    assert_ne!(done_supervisor["denied"], done_researcher["denied"]);
}

// =========================================================================
// ζ.29 — Attenuation violation: can't grant what you don't have
// =========================================================================

#[tokio::test]
async fn restrict_cannot_amplify_authority() {
    let world = build_world();

    // First, restrict database to READ-only. The result is a
    // database_readonly slot that holds only READ.
    let readonly_slot = world.space.restrict::<DatabaseResource>(
        world.database_slot,
        CapabilityRights { operations: OperationRights::READ, timeout_ms: 5000 },
        "database_readonly".into(),
    ).expect("first restrict ok");

    // Now try to derive from database_readonly a slot that
    // has READ+WRITE. The readonly slot doesn't hold WRITE,
    // so this should fail with AttenuationViolation.
    let result = world.space.restrict::<DatabaseResource>(
        readonly_slot,
        CapabilityRights {
            operations: OperationRights::READ | OperationRights::WRITE,
            timeout_ms: 5000,
        },
        "database_amplified".into(),
    );
    assert!(result.is_err(), "amplification must be rejected");
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.contains("AttenuationViolation"),
        "expected AttenuationViolation, got: {err_msg}"
    );
}

// =========================================================================
// ζ.30 — Same parent, multiple restricted delegations
// =========================================================================

#[tokio::test]
async fn supervisor_can_delegate_multiple_slices_to_separate_consumers() {
    let world = build_world();

    // Supervisor restricts database twice: once for Researcher
    // (READ-only), once for Writer (READ+WRITE).
    let researcher_slice = world.space.restrict::<DatabaseResource>(
        world.database_slot,
        CapabilityRights { operations: OperationRights::READ, timeout_ms: 5000 },
        "db_researcher".into(),
    ).expect("restrict to researcher");

    let writer_slice = world.space.restrict::<DatabaseResource>(
        world.database_slot,
        CapabilityRights {
            operations: OperationRights::READ | OperationRights::WRITE,
            timeout_ms: 5000,
        },
        "db_writer".into(),
    ).expect("restrict to writer");

    // The two restricted slots must be DIFFERENT (no aliasing).
    assert_ne!(researcher_slice.raw(), writer_slice.raw());

    // Each slice's held ops reflects its restriction.
    let res_cap = world.space.lookup_typed::<DatabaseResource>(researcher_slice).expect("res cap");
    let wri_cap = world.space.lookup_typed::<DatabaseResource>(writer_slice).expect("wri cap");
    let res_ops = res_cap.operations();
    let wri_ops = wri_cap.operations();
    assert!(res_ops.contains(OperationRights::READ));
    assert!(!res_ops.contains(OperationRights::WRITE));
    assert!(wri_ops.contains(OperationRights::READ));
    assert!(wri_ops.contains(OperationRights::WRITE));

    // Researcher tries to write → fails at the kernel level
    // (cap.invoke_op refuses bits not held).
    let write_req = json!({ "op": "write", "key": "k", "value": "v" });
    let res_write = res_cap.invoke_op(OperationRights::WRITE, write_req.clone());
    assert!(res_write.is_err(), "researcher write must fail; got {:?}", res_write);

    // Writer writes successfully.
    let wri_write = wri_cap.invoke_op(OperationRights::WRITE, write_req);
    assert!(wri_write.is_ok(), "writer write must succeed: {:?}", wri_write);

    // Suppress unused program unused-import warning (kept for
    // future cross-program tests).
    let _ = std::marker::PhantomData::<ProgramStep>;
}

// =========================================================================
// Edge test 3 — restrict on an already-revoked slot returns error
// =========================================================================

#[tokio::test]
async fn restrict_on_already_revoked_cap_returns_error() {
    use odyssey::capability::{CapabilityRights, OperationRights};

    let world = build_world();

    // Revoke the database slot first.
    let removed = world.space.revoke(world.database_slot);
    assert!(removed, "first revoke should succeed");

    // Restrict on the revoked slot must return an error —
    // the slot is empty, so the lookup_typed inside restrict
    // returns CapabilityError::SlotEmpty.
    let result = world.space.restrict::<DatabaseResource>(
        world.database_slot,
        CapabilityRights {
            operations: OperationRights::READ,
            timeout_ms: 5000,
        },
        "after_revoke".into(),
    );
    assert!(result.is_err(), "restrict on revoked slot must fail; got {:?}", result);
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(
        err_msg.contains("SlotEmpty") || err_msg.contains("empty") || err_msg.contains("revoked"),
        "expected SlotEmpty-ish error; got {err_msg}"
    );
}
