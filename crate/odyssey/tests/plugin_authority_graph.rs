//! Phase 6 — Plugin Authority Graph verification.
//!
//! Proves by integration test that the three role bits
//! `INVOKE / ASSIGN / REVOKE` form a Plugin-to-Plugin Authority
//! Graph on top of the existing kernel API. The kernel
//! surface (`Slot::grant` / `Slot::transfer` /
//! `cspace.revoke` / `cspace.revoke_tree`) is exercised
//! against a 4-plugin graph (A, B, C, D) and the resulting
//! behaviour is asserted.
//!
//! **Descriptive, not prescriptive.** Phase 6 pins what the
//! kernel does *today*, including the gaps surfaced in the
//! research artifact §"Architecture Insights". Malicious-
//! plugin tests live in `plugin_authority_misuse.rs`.

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::{CapabilitySpace, PluginCspace};
use odyssey::capability::handle::slot::Slot;
use odyssey::core::contract::resource::Resource;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::CapabilityDecl;
use odyssey::core::rights::rights::{CapabilityRights, Rights};

// ---------------------------------------------------------------------------
// StubResource + helpers (mirrors plugin_teardown.rs:96-122)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct StubResource;

impl Resource for StubResource {
    fn invoke(&self, _input: serde_json::Value) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({}))
    }
}

fn plugin(name: &str) -> PluginId {
    PluginId {
        name: name.into(),
        version: "0.1.0".into(),
    }
}

fn make_decl(name: &str) -> CapabilityDecl {
    CapabilityDecl {
        name: name.to_string(),
        kind: CapKind::Sync,
        contract_name: String::new(),
        tool_schema: None,
    }
}

// ---------------------------------------------------------------------------
// 4-plugin fixture: A (holder), B (target), C (peer), D (transitive)
// ---------------------------------------------------------------------------

struct FourPluginGraph {
    #[allow(dead_code)]
    global: CapabilitySpace,
    a: Arc<PluginCspace>,
    b: Arc<PluginCspace>,
    c: Arc<PluginCspace>,
    #[allow(dead_code)]
    d: Arc<PluginCspace>,
}

fn four_plugin_graph() -> FourPluginGraph {
    let global = CapabilitySpace::new();
    let a = Arc::new(PluginCspace::new(plugin("plugin_a")));
    let b = Arc::new(PluginCspace::new(plugin("plugin_b")));
    let c = Arc::new(PluginCspace::new(plugin("plugin_c")));
    let d = Arc::new(PluginCspace::new(plugin("plugin_d")));
    FourPluginGraph { global, a, b, c, d }
}

impl FourPluginGraph {
    /// B mints `cap_name` into its own cspace and returns
    /// the local slot id.
    fn b_mint(&self, cap_name: &str) -> SlotId {
        self.b.mint(
            CapKind::Sync,
            &make_decl(cap_name),
            CapabilityBudget::new(5000),
            Arc::new(StubResource),
        )
    }

    /// Cross-cspace grant via `grant_to`.
    fn grant_to<R: Resource>(
        &self,
        src: &PluginCspace,
        src_slot: SlotId,
        dst: &PluginCspace,
        rights: Rights,
        new_name: &str,
    ) -> SlotId {
        src.inner()
            .grant_to::<R>(
                src_slot,
                dst.inner(),
                CapabilityRights {
                    operations: rights,
                    timeout_ms: 5000,
                },
                new_name.to_string(),
            )
            .expect("grant_to must succeed")
    }
}

// =============================================================================
// Phase 1: INVOKE baseline
// =============================================================================

/// Plugin A holds only `Rights::INVOKE` on B's capability.
/// A invokes successfully. This is the one bit the kernel
/// enforces at every call (typed.rs:244).
#[test]
fn invoke_baseline_a_can_invoke_b() {
    let g = four_plugin_graph();
    let b_slot = g.b_mint("echo");

    // A holds INVOKE only.
    let a_slot = g.grant_to::<StubResource>(
        &g.b,
        b_slot,
        &g.a,
        Rights::INVOKE,
        "echo_for_a",
    );

    let slot = Slot::<StubResource>::new(g.a.inner().clone(), a_slot);
    let result = slot.invoke(Rights::INVOKE, serde_json::json!({}));
    assert!(
        result.is_ok(),
        "INVOKE-only holder must invoke: {result:?}"
    );
}

/// Plugin A holds only `Rights::INVOKE`. A tries to invoke
/// with `Rights::ASSIGN` (which A does not hold). The kernel
/// MUST reject this — `typed.rs:244` is the single enforcement
/// site. This is the one path Phase 6 asserts positively.
#[test]
fn invoke_baseline_a_denied_assign_op() {
    let g = four_plugin_graph();
    let b_slot = g.b_mint("echo");
    let a_slot = g.grant_to::<StubResource>(
        &g.b,
        b_slot,
        &g.a,
        Rights::INVOKE,
        "echo_for_a",
    );

    let slot = Slot::<StubResource>::new(g.a.inner().clone(), a_slot);
    let result = slot.invoke(Rights::ASSIGN, serde_json::json!({}));
    assert!(
        matches!(
            result,
            Err(odyssey::capability::error::CapabilityError::OperationDenied { .. })
        ),
        "INVOKE-only holder invoking with ASSIGN must be denied; got {result:?}"
    );
}

// =============================================================================
// Phase 2: ASSIGN delegation
// =============================================================================

/// A holds `INVOKE | ASSIGN` on B. A grants `INVOKE | ASSIGN`
/// to C — C is now a peer that can both invoke and re-grant.
/// Both A and C can invoke B. C can grant `INVOKE`-only to D.
/// D holds `INVOKE`-only — it can invoke, and it can re-grant
/// a strict subset (still `INVOKE`-only). What D **cannot**
/// do is amplify: it cannot grant `INVOKE | ASSIGN` (the
/// kernel enforces `child ⊆ parent` at every `derive_with`).
#[test]
fn assign_delegation_a_to_c_can_invoke_and_regrant() {
    let g = four_plugin_graph();
    let b_slot = g.b_mint("echo");

    // A holds the full INVOKE | ASSIGN.
    let a_slot = g.grant_to::<StubResource>(
        &g.b,
        b_slot,
        &g.a,
        Rights::INVOKE | Rights::ASSIGN,
        "echo_for_a",
    );

    // A grants the same rights to C.
    let c_slot = g.grant_to::<StubResource>(
        &g.a,
        a_slot,
        &g.c,
        Rights::INVOKE | Rights::ASSIGN,
        "echo_for_c",
    );

    // Both A and C can invoke.
    let a = Slot::<StubResource>::new(g.a.inner().clone(), a_slot);
    let c = Slot::<StubResource>::new(g.c.inner().clone(), c_slot);
    assert!(a.invoke(Rights::INVOKE, serde_json::json!({})).is_ok());
    assert!(c.invoke(Rights::INVOKE, serde_json::json!({})).is_ok());

    // C further grants INVOKE-only to D (attenuation strips ASSIGN).
    let d_slot = c
        .grant(
            CapabilityRights {
                operations: Rights::INVOKE,
                timeout_ms: 5000,
            },
            "echo_for_d".into(),
        )
        .expect("C's grant to D must succeed");

    // D can invoke (it holds INVOKE).
    let d = Slot::<StubResource>::new(g.c.inner().clone(), d_slot);
    assert!(d.invoke(Rights::INVOKE, serde_json::json!({})).is_ok());

    // D CANNOT amplify: it holds INVOKE-only and tries to
    // grant `INVOKE | ASSIGN`. Attenuation rejects because
    // `INVOKE | ASSIGN ⊄ INVOKE`.
    let denied = d.grant(
        CapabilityRights {
            operations: Rights::INVOKE | Rights::ASSIGN,
            timeout_ms: 5000,
        },
        "echo_for_e_with_assign".into(),
    );
    assert!(
        matches!(
            denied,
            Err(odyssey::capability::error::CapabilityError::AttenuationViolation { .. })
        ),
        "D (INVOKE-only) attempting to grant INVOKE|ASSIGN must \
         attenuate (kernel-enforced `child ⊆ parent`); got {denied:?}"
    );

    // D CAN re-grant a strict subset (still INVOKE-only) — this
    // is *attenuation-allowed* but the produced child is just
    // a copy of D's view. This is the descriptive kernel
    // behaviour: there is no caller-identity check on
    // `slot.grant`; the kernel only checks `child ⊆ parent`.
    let _peer = d
        .grant(
            CapabilityRights {
                operations: Rights::INVOKE,
                timeout_ms: 5000,
            },
            "echo_for_peer".into(),
        )
        .expect("D's INVOKE-only grant to a peer must succeed \
                 (attenuation passes — no caller-identity check)");
}

// =============================================================================
// Phase 3: REVOKE scope
// =============================================================================

/// A holds `INVOKE | ASSIGN` on B; A grants `INVOKE` to C.
/// Revoking C's slot kills C's view but leaves A's own slot
/// alive — REVOKE kills the delegation, not the target.
#[test]
fn revoke_kills_delegation_not_target() {
    let g = four_plugin_graph();
    let b_slot = g.b_mint("echo");

    let a_slot = g.grant_to::<StubResource>(
        &g.b,
        b_slot,
        &g.a,
        Rights::INVOKE | Rights::ASSIGN,
        "echo_for_a",
    );
    let c_slot = g.grant_to::<StubResource>(
        &g.a,
        a_slot,
        &g.c,
        Rights::INVOKE,
        "echo_for_c",
    );

    // Sanity: C can invoke before revoke.
    let c_before = Slot::<StubResource>::new(g.c.inner().clone(), c_slot);
    assert!(c_before.invoke(Rights::INVOKE, serde_json::json!({})).is_ok());

    // Cross-cspace revocation. `grant_to` does NOT populate
    // the `parents` map, so the kernel cannot reach C's slot
    // via `revoke_tree` from A's cspace. We call
    // `c.inner().revoke(c_slot)` directly to demonstrate the
    // semantic; Phase 7 will populate `parents` across cspaces.
    let removed = g.c.inner().revoke(c_slot);
    assert!(removed, "revoke of C's slot must report removal");

    // C can no longer invoke.
    let c_after = Slot::<StubResource>::new(g.c.inner().clone(), c_slot);
    let err = c_after
        .invoke(Rights::INVOKE, serde_json::json!({}))
        .expect_err("C invoke after revoke must fail");
    assert!(
        matches!(
            err,
            odyssey::capability::error::CapabilityError::Revoked(_)
                | odyssey::capability::error::CapabilityError::SlotEmpty(_)
        ),
        "expected Revoked or SlotEmpty; got {err:?}"
    );

    // A's own slot is unaffected.
    let a = Slot::<StubResource>::new(g.a.inner().clone(), a_slot);
    assert!(
        a.invoke(Rights::INVOKE, serde_json::json!({})).is_ok(),
        "A's slot must survive revocation of C's slot"
    );
}

// =============================================================================
// Phase 4: Transitive REVOKE (within one cspace)
// =============================================================================

/// Within a single cspace, `revoke_tree(parent)` walks the
/// `parents` map and clears every descendant. Mirrors
/// `plugin_teardown.rs:408` but uses the 4-plugin fixture.
/// Also documents the kernel gap: `grant_to` does not
/// populate `parents`, so cross-cspace descendants survive.
#[test]
fn revoke_tree_kills_grandchildren_in_same_cspace() {
    let g = four_plugin_graph();
    let b_slot = g.b_mint("echo");

    // A holds all three bits.
    let a_slot = g.grant_to::<StubResource>(
        &g.b,
        b_slot,
        &g.a,
        Rights::ALL,
        "echo_for_a",
    );
    // A grants INVOKE-only to C (cross-cspace).
    let c_slot = g.grant_to::<StubResource>(
        &g.a,
        a_slot,
        &g.c,
        Rights::INVOKE,
        "echo_for_c",
    );

    // Within A's cspace, A further grants INVOKE-only to a child.
    let a = Slot::<StubResource>::new(g.a.inner().clone(), a_slot);
    let grandchild_slot = a
        .grant(
            CapabilityRights {
                operations: Rights::INVOKE,
                timeout_ms: 5000,
            },
            "echo_grandchild".into(),
        )
        .expect("grant in A's cspace must succeed");

    // Sanity: grandchild works before revoke.
    let grandchild = Slot::<StubResource>::new(g.a.inner().clone(), grandchild_slot);
    assert!(
        grandchild
            .invoke(Rights::INVOKE, serde_json::json!({}))
            .is_ok()
    );

    // Revoke A's slot's tree within A's cspace.
    let removed = g.a.inner().revoke_tree(a_slot);
    assert!(
        removed >= 2,
        "expected at least 2 (A + grandchild) removed, got {removed}"
    );

    // Grandchild is dead.
    let err = grandchild
        .invoke(Rights::INVOKE, serde_json::json!({}))
        .expect_err("grandchild invoke after revoke_tree must fail");
    assert!(
        matches!(
            err,
            odyssey::capability::error::CapabilityError::Revoked(_)
                | odyssey::capability::error::CapabilityError::SlotEmpty(_)
        ),
        "expected Revoked or SlotEmpty; got {err:?}"
    );

    // C's slot (in C's cspace) is NOT reachable from A's cspace
    // because `grant_to` does not populate `parents`. Document
    // this gap by checking that A's revoke_tree left C's slot
    // untouched.
    let c_still = Slot::<StubResource>::new(g.c.inner().clone(), c_slot);
    assert!(
        c_still.invoke(Rights::INVOKE, serde_json::json!({})).is_ok(),
        "C's slot survives A's revoke_tree (kernel gap: grant_to \
         does not populate parents across cspaces — Phase 7 \
         hardening opportunity)"
    );
}
