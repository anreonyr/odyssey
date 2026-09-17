//! Rights attenuation invariants (slice 1 of the INVOKE/ASSIGN/REVOKE
//! redesign). Locks down:
//!   - `Rights::contains` is monotone and exact (the bitflags-
//!     generated `contains` is the role-typed attenuation check).
//!   - `Rights::intersect` is the meet.
//!   - `From<OperationRights>` collapses the legacy four-bit space
//!     into the new three-bit role space (conservative mapping).
//!   - `From<Rights>` projects back to the legacy space (only used
//!     during the migration window).
//!   - `CapabilityRights::contains` semantics are preserved across
//!     the migration.

use odyssey::core::rights::rights::{CapabilityRights, OperationRights, Rights};

#[test]
fn rights_contains_is_exact_superset() {
    // `contains` is provided by the `bitflags!` macro and is
    // exactly the role-typed attenuation check (`self ⊇ other`).
    assert!(Rights::ALL.contains(Rights::INVOKE));
    assert!(Rights::ALL.contains(Rights::ASSIGN));
    assert!(Rights::ALL.contains(Rights::REVOKE));
    assert!(Rights::ALL.contains(Rights::ALL));
    assert!(!Rights::INVOKE.contains(Rights::ASSIGN));
    assert!(!Rights::INVOKE.contains(Rights::REVOKE));
    assert!(!Rights::empty().contains(Rights::INVOKE));
}

#[test]
fn rights_intersect_is_the_meet() {
    let a = Rights::INVOKE | Rights::ASSIGN;
    let b = Rights::ASSIGN | Rights::REVOKE;
    assert_eq!(a.intersect(&b), Rights::ASSIGN);

    let c = Rights::INVOKE;
    assert_eq!(c.intersect(&Rights::ALL), Rights::INVOKE);
    assert_eq!(c.intersect(&Rights::empty()), Rights::empty());

    // INVOKE ∩ REVOKE is empty — disjoint role axes.
    assert_eq!(Rights::INVOKE.intersect(&Rights::REVOKE), Rights::empty());
}

#[test]
fn rights_default_is_all() {
    assert_eq!(Rights::default(), Rights::ALL);
}

#[test]
fn rights_three_bits_match_role_axes() {
    // INVOKE / ASSIGN / REVOKE are distinct axes — each subset
    // represents a meaningful role. There are 2^3 = 8 subsets
    // (the empty set is the degenerate "no authority" cap that
    // exists but is useless). This test enumerates the 7
    // role-typed non-empty ones to keep the vocabulary tight.
    let roles = [
        Rights::INVOKE,
        Rights::ASSIGN,
        Rights::REVOKE,
        Rights::INVOKE | Rights::ASSIGN,
        Rights::INVOKE | Rights::REVOKE,
        Rights::ASSIGN | Rights::REVOKE,
        Rights::ALL,
    ];
    for bits in roles {
        assert!(!bits.is_empty(), "role must be non-empty");
    }
}

#[test]
fn legacy_operation_rights_fold_into_invoke_or_revoke() {
    // READ | WRITE | EXECUTE all map to INVOKE (operation-typed
    // verbs collapse into the role-typed traversal authority).
    assert_eq!(Rights::from(OperationRights::READ), Rights::INVOKE);
    assert_eq!(Rights::from(OperationRights::WRITE), Rights::INVOKE);
    assert_eq!(Rights::from(OperationRights::EXECUTE), Rights::INVOKE);
    // ADMIN maps to REVOKE (lifecycle authority is the closest
    // legacy analog under the role-typed schema).
    assert_eq!(Rights::from(OperationRights::ADMIN), Rights::REVOKE);
    // ALL = READ | WRITE | EXECUTE | ADMIN. Three of those bits
    // fold into INVOKE; ADMIN folds into REVOKE. The legacy schema
    // has no ASSIGN analog, so the projection is INVOKE | REVOKE
    // (NOT Rights::ALL — that's documented in design R1.1 as
    // the lossy collapse across the migration window).
    assert_eq!(
        Rights::from(OperationRights::ALL),
        Rights::INVOKE | Rights::REVOKE
    );
}

#[test]
fn legacy_projection_round_trips_through_assign_and_revoke() {
    // INVOKE → READ | WRITE | EXECUTE (the closest legacy analog:
    // all three operation verbs that the old schema distinguished
    // collapse into one role-typed bit).
    let projected: OperationRights = Rights::INVOKE.into();
    assert!(projected.contains(OperationRights::READ));
    assert!(projected.contains(OperationRights::WRITE));
    assert!(projected.contains(OperationRights::EXECUTE));
    // ASSIGN → ADMIN (legacy closest analog; conservative).
    let projected: OperationRights = Rights::ASSIGN.into();
    assert!(projected.contains(OperationRights::ADMIN));
    // REVOKE → ADMIN.
    let projected: OperationRights = Rights::REVOKE.into();
    assert!(projected.contains(OperationRights::ADMIN));
}

#[test]
fn capability_rights_containment_unchanged() {
    // CapabilityRights::contains semantics preserved across the
    // migration: operations ⊇ operations' AND timeout_ms ≥ timeout_ms'.
    // Slice 1 keeps `operations: OperationRights`; slice 2 flips it
    // to `Rights`. The test uses the legacy type to validate the
    // unchanged attenuation contract.
    let parent = CapabilityRights {
        operations: OperationRights::ALL,
        timeout_ms: 5000,
    };
    let child = CapabilityRights {
        operations: OperationRights::EXECUTE,
        timeout_ms: 3000,
    };
    assert!(parent.contains(&child));
    assert!(!child.contains(&parent));
}

#[test]
fn capability_rights_timeout_monotone() {
    // A child with a LARGER timeout than its parent must NOT
    // satisfy `contains`. This is the budget-axis half of the
    // attenuation contract; the operations-axis half is covered
    // above.
    let parent = CapabilityRights {
        operations: OperationRights::ALL,
        timeout_ms: 1000,
    };
    let child = CapabilityRights {
        operations: OperationRights::EXECUTE,
        timeout_ms: 5000, // exceeds parent's 1000ms ceiling
    };
    assert!(!parent.contains(&child));
}