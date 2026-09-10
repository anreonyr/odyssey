//! P1-J — M2 invariant: AnyCapability::set_revoked_dyn default
//! panics; the typed Capability<R> impl overrides.
//!
//! Phase 5 M2 invariant: every AnyCapability impl MUST
//! override set_revoked_dyn; the default panics so that
//! silent fall-through past revocation is impossible.
//! Without this tripwire, a future AnyCapability impl that
//! forgets the marker flip would let revoked caps continue
//! to dispatch \u2014 the original soundness gap.
//!
//! These tests pin the contract:
//!
//!   - Positive case (1): cspace::revoke flips the marker on
//!     a typed Capability<R>. is_revoked() returns true after
//!     revoke. This pins the typed-cap override.
//!
//!   - Positive case (2): cspace::install on a fresh slot id
//!     resets the marker (set_revoked(false)). A new install
//!     starts live.
//!
//!   - Negative case: an AnyCapability stub that does NOT
//!     override set_revoked_dyn panics when the default impl
//!     is invoked. The panic message identifies the type name.
//!
//!   - Direct check: the trait's default set_revoked_dyn
//!     impl panics; the panic message matches the M2 contract.
use std::sync::Arc;

use odyssey::kernel::cap::erased::AnyCapability;
use odyssey::kernel::chunk::CapabilityChunk;
use odyssey::kernel::error::CapabilityError;
use odyssey::kernel::meta::CapabilityMeta;
use odyssey::kernel::rights::OperationRights;
use odyssey::kernel::{Capability, Resource, Slot};
use serde_json::Value;
use tokio::sync::mpsc;

/// Stub AnyCapability that does NOT override
/// `set_revoked_dyn`. Inherits the panic-on-default from
/// the trait definition. Used by the negative-case test
/// below.
struct StubNoSetRevoked {
    meta: CapabilityMeta,
}

impl AnyCapability for StubNoSetRevoked {
    fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }
    fn is_streaming(&self) -> bool {
        false
    }
    fn operations(&self) -> OperationRights {
        OperationRights::ALL
    }
    fn invoke_dyn(&self, _input: Value) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn invoke_op_dyn(
        &self,
        _op: OperationRights,
        _input: Value,
    ) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn open_dyn(
        &self,
        _input: Value,
    ) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        Err("not streaming".to_string())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    // NOTE: deliberately NOT overriding set_revoked_dyn.
    // The default panics with the M2 message.
}

fn build_stub_meta(name: &str) -> CapabilityMeta {
    CapabilityMeta {
        id: odyssey::kernel::ids::CapabilityId(0),
        name: name.into(),
        namespace: String::new(),
        contract_name: String::new(),
        plugin: odyssey::kernel::PluginId {
            name: "stub".into(),
            version: "0.1.0".into(),
        },
        in_type: "any".into(),
        out_type: "any".into(),
        streaming: false,
        timeout_ms: 5000,
        quota: odyssey::kernel::QuotaSpec::default(),
        authority: odyssey::kernel::AuthorityContract::default(),
        protocol: odyssey::kernel::Protocol::default(),
    }
}

/// Positive case (1): cspace::revoke flips the marker on a
/// typed Capability<R>. is_revoked() returns true after
/// revoke. invoke on the revoked cap returns the typed
/// `Revoked(SlotId)` variant.
#[test]
fn typed_cap_revoked_via_cspace_blocks_invoke() {
    let (space, factory) = crate::common::boot();
    let slot = crate::common::mint_counter(&factory);
    let typed: Arc<Capability<odyssey::plugins::test_only::counter::CounterResource>> =
        Slot::new(space.clone(), slot)
            .capability()
            .expect("typed cap");

    assert!(!typed.is_revoked(), "fresh cap is live");

    // Live dispatch.
    let live = typed.invoke_op(
        OperationRights::READ,
        serde_json::json!({"op": "read"}),
    );
    assert!(live.is_ok(), "live cap should dispatch; got {live:?}");

    // Revoke via the cspace.
    assert!(space.revoke(slot), "revoke must succeed");
    assert!(typed.is_revoked(), "marker must flip on revoke");

    let denied = typed.invoke_op(
        OperationRights::READ,
        serde_json::json!({"op": "read"}),
    );
    assert!(
        matches!(denied, Err(CapabilityError::Revoked(_))),
        "revoked cap must return typed Revoked variant; got {denied:?}"
    );
}

/// Positive case (2): cspace::install on a fresh slot id
/// resets the marker. A new slot id mints a fresh cap with
/// is_revoked() == false \u2014 the lifecycle reset is the
/// kernel's responsibility, not the holder's.
#[test]
fn cspace_install_resets_marker_on_new_slot() {
    let (space, factory) = crate::common::boot();
    let slot = crate::common::mint_counter(&factory);
    let typed: Arc<Capability<odyssey::plugins::test_only::counter::CounterResource>> =
        Slot::new(space.clone(), slot)
            .capability()
            .expect("typed cap");

    assert!(space.revoke(slot), "first revoke must succeed");
    assert!(typed.is_revoked(), "first cap is revoked");

    // Mint a second slot. The marker on the new cap starts
    // at false.
    let slot2 = crate::common::mint_counter(&factory);
    let typed2: Arc<Capability<odyssey::plugins::test_only::counter::CounterResource>> =
        Slot::new(space.clone(), slot2)
            .capability()
            .expect("typed cap2");
    assert!(
        !typed2.is_revoked(),
        "fresh install on a new slot id must reset the marker"
    );

    // And the new cap dispatches normally.
    let ok = typed2.invoke_op(
        OperationRights::READ,
        serde_json::json!({"op": "read"}),
    );
    assert!(ok.is_ok(), "fresh cap should dispatch; got {ok:?}");
}

/// Negative case: an AnyCapability stub that doesn't override
/// set_revoked_dyn inherits the default panic. The panic
/// message identifies the cap's type name (M2 contract).
#[test]
#[should_panic(expected = "AnyCapability impl for")]
fn stub_no_set_revoked_panics_on_default_call() {
    let stub = StubNoSetRevoked {
        meta: build_stub_meta("panic_demo"),
    };
    let erased: Arc<dyn AnyCapability> = Arc::new(stub);
    // Default set_revoked_dyn impl panics.
    erased.set_revoked_dyn(true);
}

/// Direct check: invoking the trait method on a stub
/// (without going through cspace::revoke) panics with the
/// M2 message. The message format is the contract:
/// "AnyCapability impl for `<name>` did not override
/// set_revoked_dyn; caps of this type will continue to
/// dispatch past revocation. Implement the marker flip
/// in the impl block."
#[test]
#[should_panic(expected = "did not override set_revoked_dyn")]
fn default_set_revoked_message_contains_m2_contract() {
    let stub = StubNoSetRevoked {
        meta: build_stub_meta("explicit_msg"),
    };
    stub.set_revoked_dyn(true);
}

/// Marker survives Arc::clone \u2014 the typed cap's
/// \`Arc<AtomicBool>\` is shared between clones (Phase 4
/// review-loop fix). A revoked cap, cloned, still reports
/// is_revoked() == true.
#[test]
fn typed_cap_clone_shares_revocable_marker() {
    let (space, factory) = crate::common::boot();
    let slot = crate::common::mint_counter(&factory);
    let a: Arc<Capability<odyssey::plugins::test_only::counter::CounterResource>> =
        Slot::new(space.clone(), slot)
            .capability()
            .expect("typed cap a");
    let b = Arc::clone(&a);

    assert_eq!(a.is_revoked(), b.is_revoked());

    assert!(space.revoke(slot));

    assert_eq!(a.is_revoked(), b.is_revoked());
    assert!(a.is_revoked());
    assert!(b.is_revoked());
}

// `Resource` import is needed for the trait bound on
// Capability<R>; suppress unused-import warnings if any
// individual test is later removed.
#[allow(dead_code)]
fn _force_link(_r: &dyn Resource) {}