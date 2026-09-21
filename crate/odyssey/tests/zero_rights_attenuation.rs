//! Zero-rights attenuation rejection (DI Phase 21).
//!
//! Pins the new precondition at `derive_with`, `grant_to`, and
//! `install_derived`: a child cap whose `CapabilityRights::operations`
//! is `Rights::empty()` is rejected at construction time with
//! `CapabilityError::AttenuationViolation`. Previously the empty
//! rights set was installable and silently failed every invoke via
//! `Capability::invoke`'s `OperationDenied` check. The boot-time
//! loud fail turns a silent runtime fault into a clear
//! configuration error.

use std::sync::Arc;

use odyssey::capability::enforce::quota::CapabilityBudget;
use odyssey::capability::enforce::space::{CapabilitySpace, PluginCspace};
use odyssey::capability::error::CapabilityError;
use odyssey::capability::handle::cap::Capability;
use odyssey::core::contract::resource::Resource;
use odyssey::core::identity::ids::SlotId;
use odyssey::core::identity::kind::CapKind;
use odyssey::core::rights::rights::{CapabilityRights, Rights};
use serde_json::Value;

// --- Stub resource (mirrors plugin_teardown.rs:79) ---

#[derive(Debug)]
struct StubResource;

impl Resource for StubResource {
    fn invoke(&self, _input: Value) -> Result<Value, String> {
        Ok(serde_json::json!({}))
    }
}

// --- Helper: mint a single cap with full rights. Returns the
// global-cspace slot id so the test can exercise attenuation
// paths that take a SlotId as input. ---
fn mint_with_full_rights(space: &CapabilitySpace) -> SlotId {
    use odyssey::core::identity::ids::PluginId;
    let plugin = PluginId {
        name: "zero_rights_test".into(),
        version: "0.1.0".into(),
    };
    use odyssey::core::manifest::manifest::CapabilityDecl;
    let decl = CapabilityDecl {
        name: "stub".into(),
        kind: CapKind::Sync,
        contract_name: "stub".into(),
        tool_schema: None,
        priority: None,
    };
    let pc = PluginCspace::new(plugin.clone());
    let local_slot = pc.mint(
        CapKind::Sync,
        &decl,
        CapabilityBudget::new(5000),
        Arc::new(StubResource),
    );
    pc.inner()
        .grant_to::<StubResource>(
            local_slot,
            space,
            CapabilityRights {
                operations: Rights::ALL,
                timeout_ms: 5000,
            },
            "stub".into(),
        )
        .expect("full-rights grant should succeed")
}

#[test]
fn derive_with_rejects_zero_rights_child() {
    let space = CapabilitySpace::new();
    let stub = mint_with_full_rights(&space);

    let result = space.restrict::<StubResource>(
        stub,
        CapabilityRights {
            operations: Rights::empty(),
            timeout_ms: 5000,
        },
        "empty_child".into(),
    );
    assert!(
        matches!(result, Err(CapabilityError::AttenuationViolation { .. })),
        "derive_with must reject Rights::empty() child; got {:?}",
        result
    );
}

#[test]
fn grant_to_rejects_zero_rights_child() {
    let space = CapabilitySpace::new();
    let stub = mint_with_full_rights(&space);

    // Manually walk the same path grant_to takes: source cap,
    // attenuation check, derive, bind_slot, install.
    let source: Arc<Capability<StubResource>> = space
        .lookup_typed::<StubResource>(stub)
        .expect("source lookup");

    let rights = CapabilityRights {
        operations: Rights::empty(),
        timeout_ms: 5000,
    };
    let held = source.rights();
    // Attenuation check passes (empty ⊆ ALL).
    assert!(held.contains(&rights));

    // Re-run grant_to inline to exercise the new precondition:
    let target = CapabilitySpace::new();
    let result = space.grant_to::<StubResource>(stub, &target, rights, "empty_grant".into());
    assert!(
        matches!(result, Err(CapabilityError::AttenuationViolation { .. })),
        "grant_to must reject Rights::empty() child; got {:?}",
        result
    );
}

#[test]
fn rights_empty_is_actually_empty() {
    // Sanity check on the Rights bitflag — make sure
    // `Rights::empty()` returns a value with no bits set and
    // is NOT `Rights::ALL`.
    let e = Rights::empty();
    assert_eq!(e.bits(), 0);
    assert!(!e.contains(Rights::INVOKE));
    assert!(!e.contains(Rights::ASSIGN));
    assert!(!e.contains(Rights::REVOKE));
    assert!(!e.contains(Rights::ALL));
    // `contains(other)` is true iff every bit of `other` is in
    // `self`; `empty().contains(empty()) == true` is the
    // invariant the new precondition breaks at the
    // attenuation sites.
    assert!(e.contains(Rights::empty()));
}
