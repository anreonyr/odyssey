//! Phase 6 — malicious-plugin attack surface.
//!
//! Pins the kernel's current behaviour against four
//! adversarial scenarios. Each test is annotated with
//! the kernel gap it surfaces and named as a Phase 7
//! hardening opportunity.
//!
//! **Descriptive, not prescriptive.** These tests pass
//! against today's kernel. If a future kernel change adds
//! the missing enforcement, these tests must be updated
//! to assert the new behaviour (the gap comments make the
//! change visible).

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::{CapabilitySpace, PluginCspace};
use odyssey::capability::handle::cap::Capability;
use odyssey::capability::handle::slot::Slot;
use odyssey::core::contract::resource::Resource;
use odyssey::core::identity::ids::{PluginId, SlotId};
use odyssey::core::identity::kind::CapKind;
use odyssey::core::manifest::manifest::CapabilityDecl;
use odyssey::core::rights::rights::{CapabilityRights, Rights};

// ---------------------------------------------------------------------------
// Resource types — StubResource (the legitimate target) + OtherResource
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct StubResource;

impl Resource for StubResource {
    fn invoke(&self, _input: serde_json::Value) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({}))
    }
}

#[derive(Debug)]
struct OtherResource;

impl Resource for OtherResource {
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
        priority: None,
    }
}

// =============================================================================
// Attack 1: Forge a SlotId
// =============================================================================

/// A malicious plugin forges a `SlotId` by allocating one
/// (which is public via `CapabilitySpace::allocate`) and
/// tries to look it up. The kernel MUST refuse — the slot
/// is not installed. This is one of the **kernel-enforced**
/// properties today: `lookup_typed` / `lookup_erased`
/// return `None` when the slot is empty.
#[test]
fn forged_slot_id_is_rejected() {
    let _global = CapabilitySpace::new();
    let attacker = Arc::new(PluginCspace::new(plugin("attacker")));

    // Attacker forges a slot id.
    let forged = attacker.inner().allocate();

    // Kernel rejects: nothing installed at this slot.
    let result: Option<Arc<Capability<StubResource>>> =
        attacker.inner().lookup_typed::<StubResource>(forged);
    assert!(
        result.is_none(),
        "forged slot id must not resolve to a typed capability"
    );

    // Even the erased path rejects.
    let erased = attacker.inner().lookup_erased(forged);
    assert!(erased.is_none(), "forged slot id must not resolve at all");
}

// =============================================================================
// Attack 2: Cross-plugin name lookup
// =============================================================================

/// A malicious plugin in cspace C tries to look up a
/// capability name that lives in plugin B's cspace.
/// The kernel MUST refuse — C does not hold B's cspace.
/// **Kernel-enforced today**: per-plugin cspace isolation
/// is API-level (each plugin only holds its own
/// `Arc<PluginCspace>`); lookups are local to the cspace's
/// own slot table.
#[test]
fn cross_plugin_name_lookup_is_rejected() {
    let _global = CapabilitySpace::new();
    let victim = Arc::new(PluginCspace::new(plugin("victim")));
    let attacker = Arc::new(PluginCspace::new(plugin("attacker")));

    // Victim mints `secret_cap`.
    let _secret = victim.mint(
        CapKind::Sync,
        &make_decl("secret_cap"),
        CapabilityBudget::new(5000),
        Arc::new(StubResource),
    );

    // Attacker tries to look it up by name in *its own* cspace.
    let by_name = attacker.inner().lookup_by_name("secret_cap");
    assert!(
        by_name.is_none(),
        "cross-plugin name lookup must be rejected — attacker \
         has no view of victim's cspace"
    );

    // Attacker also cannot resolve via slot id it didn't receive.
    let typed: Option<Arc<Capability<StubResource>>> = attacker
        .inner()
        .lookup_typed::<StubResource>(SlotId::new(1));
    assert!(
        typed.is_none(),
        "attacker cannot resolve victim's slot by id"
    );
}

// =============================================================================
// Attack 3: Cross-type downcast
// =============================================================================

/// Plugin B mints `Capability<StubResource>`. A malicious
/// plugin holds the slot id (by some leaked channel) and
/// attempts to downcast it as `Capability<OtherResource>`.
/// The kernel's typed lookup MUST return `None` because the
/// underlying capability is the wrong type. **Kernel-enforced
/// today** via `as_any().downcast_ref::<Capability<R>>()`
/// at `space.rs:343`.
#[test]
fn cross_type_downcast_is_rejected() {
    let _global = CapabilitySpace::new();
    let victim = Arc::new(PluginCspace::new(plugin("victim")));

    let slot = victim.mint(
        CapKind::Sync,
        &make_decl("typed_cap"),
        CapabilityBudget::new(5000),
        Arc::new(StubResource),
    );

    // Correct downcast succeeds.
    let correct: Option<Arc<Capability<StubResource>>> =
        victim.inner().lookup_typed::<StubResource>(slot);
    assert!(correct.is_some(), "correct-type downcast must succeed");

    // Wrong downcast fails.
    let wrong: Option<Arc<Capability<OtherResource>>> =
        victim.inner().lookup_typed::<OtherResource>(slot);
    assert!(
        wrong.is_none(),
        "wrong-type downcast must return None (kernel refuses)"
    );
}

// =============================================================================
// Attack 4: Revoke without REVOKE bit
// =============================================================================

/// Plugin C holds only `Rights::INVOKE` on B's capability.
/// C invokes `cspace.revoke(slot)` — the kernel currently
/// has NO caller-identity check, so the revocation SUCCEEDS.
/// This test pins that permissive behaviour; Phase 7 will
/// add the caller-identity check and this test must be
/// updated to assert the new behaviour.
///
/// **Phase 7 hardening opportunity.** The design doc at
/// `designs/2026-09-17_16-29-49_capability-rights-invoke-assign-revoke.md`
/// Decision 3 says REVOKE is "scoped to edges the holder
/// created" — a property the kernel cannot enforce today
/// (no caller-identity).
#[test]
fn revoke_without_revoke_bit_is_currently_allowed_known_gap() {
    let _global = CapabilitySpace::new();
    let victim = Arc::new(PluginCspace::new(plugin("victim")));
    let attacker = Arc::new(PluginCspace::new(plugin("attacker")));

    // Victim mints a cap.
    let victim_slot = victim.mint(
        CapKind::Sync,
        &make_decl("victim_cap"),
        CapabilityBudget::new(5000),
        Arc::new(StubResource),
    );

    // Victim grants INVOKE-only to attacker (cross-cspace).
    let attacker_slot = victim
        .inner()
        .grant_to::<StubResource>(
            victim_slot,
            attacker.inner(),
            CapabilityRights {
                operations: Rights::INVOKE,
                timeout_ms: 5000,
            },
            "victim_cap_for_attacker".into(),
        )
        .expect("grant_to must succeed");

    // Attacker holds INVOKE-only (no REVOKE).
    let attacker_view = Slot::<StubResource>::new(attacker.inner().clone(), attacker_slot);
    assert!(
        attacker_view
            .invoke(Rights::INVOKE, serde_json::json!({}))
            .is_ok(),
        "sanity: attacker can invoke before revocation"
    );

    // Currently succeeds. When Phase 7 adds caller-identity,
    // this assertion will flip to `is_err()` and the test
    // will pass-by-rejection.
    let revoked = attacker_view.revoke();
    assert!(
        revoked,
        "kernel currently allows revoke without REVOKE bit — \
         Phase 7 hardening opportunity (caller-identity missing)"
    );

    // Victim's slot is unaffected: revoke cleared the
    // attacker's cross-cspace copy, not the source.
    let victim_view = Slot::<StubResource>::new(victim.inner().clone(), victim_slot);
    assert!(
        victim_view
            .invoke(Rights::INVOKE, serde_json::json!({}))
            .is_ok(),
        "victim's slot survives — attacker revoked its own copy, \
         not the source"
    );
}
