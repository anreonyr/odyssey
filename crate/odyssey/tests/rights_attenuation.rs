//! Rights attenuation invariants (Phase 17: INVOKE / ASSIGN / REVOKE
//! capability rights redesign).
//!
//! Locks down the post-Phase-5 contract:
//!   - `Rights::contains` is monotone and exact (the bitflags-
//!     generated `contains` is the role-typed attenuation check).
//!   - `Rights::intersect` is the meet.
//!   - `Rights::default` is `Rights::ALL` (root-cap convention).
//!   - The 2^3 - 1 non-empty role subsets each map to a
//!     meaningful role (consumer / forwarder / revoker / ...).
//!   - `CapabilityRights::contains` enforces both axes:
//!     operations ⊇ operations' AND timeout_ms ≥ timeout_ms'.

use odyssey::core::rights::rights::{CapabilityRights, Rights};

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
fn capability_rights_containment_unchanged() {
    // CapabilityRights::contains semantics: operations ⊇
    // operations' AND timeout_ms ≥ timeout_ms'. The operations
    // axis uses `Rights` directly post-Phase-5.
    let parent = CapabilityRights {
        operations: Rights::INVOKE | Rights::ASSIGN,
        timeout_ms: 5000,
    };
    let child = CapabilityRights {
        operations: Rights::INVOKE,
        timeout_ms: 3000,
    };
    assert!(parent.contains(&child));
    assert!(!child.contains(&parent));
}

#[test]
fn capability_rights_timeout_monotone() {
    // A child with a LARGER timeout than its parent must NOT
    // satisfy `contains`. Budget-axis half of the attenuation
    // contract; operations-axis covered above.
    let parent = CapabilityRights {
        operations: Rights::ALL,
        timeout_ms: 1000,
    };
    let child = CapabilityRights {
        operations: Rights::INVOKE,
        timeout_ms: 5000, // exceeds parent's 1000ms ceiling
    };
    assert!(!parent.contains(&child));
}
