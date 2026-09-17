//! Capability rights — the bitflag carried inside every `Capability<R>`
//! and every `CapabilityRights` (the attenuation shape).
//!
//! Phase 17: replaced the four-bit `OperationRights { READ, WRITE,
//! EXECUTE, ADMIN }` with the three-bit role-typed
//! `Rights { INVOKE, ASSIGN, REVOKE }`. The bit semantics are
//! role-typed, not operation-typed:
//!
//! - INVOKE — traverse an authority edge into the underlying Protocol.
//! - ASSIGN — propagate the capability to other plugins (kernel-
//!   enforces `child ⊆ parent`).
//! - REVOKE — retract the capability (scoped to edges the holder
//!   created; transitive over descendants per seL4 CNode revocation).
//!
//! Protocol operations (query / insert / start / ...) are NOT encoded
//! in `Rights`. They live in the Protocol layer; `Rights` only
//! authorises traversal, not what the traversal does.

use bitflags::bitflags;

// -----------------------------------------------------------------------
// Phase 17: INVOKE / ASSIGN / REVOKE rights.
// -----------------------------------------------------------------------

bitflags! {
    /// Per-edge authority bits. The kernel (CapabilitySpace) is the
    /// only thing that *creates* these; `Capability::invoke` consults
    /// the held rights at every call to reject an operation that was
    /// dropped by `restrict`.
    ///
    /// The three bits are the orthogonal axes of a Plugin-to-Plugin
    /// Authority edge: traversal, propagation, retraction.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Rights: u32 {
        /// Use the capability: invoke the underlying Protocol.
        const INVOKE  = 1 << 0;
        /// Propagate the capability to other plugins.
        /// The recipient's `Rights` is `child ⊆ parent`
        /// (kernel-enforced; see `CapabilityRights::contains`).
        /// The recipient does NOT inherit ASSIGN unless the
        /// grantor explicitly set it in the child's rights.
        const ASSIGN  = 1 << 1;
        /// Retract the capability. Scope: edges the holder
        /// themselves created. Transitive: revoking A→B kills
        /// B→C and onward (seL4 CNode revocation semantics).
        const REVOKE  = 1 << 2;
        /// Convenience: every bit set. Used when minting the
        /// root cap.
        const ALL     = Self::INVOKE.bits()
                      | Self::ASSIGN.bits()
                      | Self::REVOKE.bits();
    }
}

impl Default for Rights {
    fn default() -> Self {
        Self::ALL
    }
}

impl Rights {
    // `contains` is provided by the `bitflags!` macro — it
    // checks whether `self` has all bits of `other` set,
    // which is exactly the role-typed attenuation check
    // (`self ⊇ other`) we want. No custom impl needed.

    /// The largest rights ≤ `self` AND ≤ `other` — i.e. the
    /// intersection. Used when `restrict` silently clamps a
    /// caller that asks for more than it has.
    pub fn intersect(&self, other: &Rights) -> Rights {
        *self & *other
    }
}

/// Rights attached to a capability. Supplied when deriving a child via
/// `grant`, `transfer`, or `restrict`. Two dimensions:
///
/// - `operations` — what the holder may *do* (`READ`/`WRITE`/...).
///   Attenuation (`restrict`) enforces `child ⊆ parent`.
/// - `timeout_ms` — wall-clock budget per call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilityRights {
    pub operations: Rights,
    pub timeout_ms: u32,
}

impl Default for CapabilityRights {
    fn default() -> Self {
        Self {
            operations: Rights::ALL,
            timeout_ms: 5000,
        }
    }
}

impl CapabilityRights {
    /// `self ⊇ other` — every bit and every budget ceiling in `other`
    /// is also present in `self`. The CSpace rejects any child whose
    /// rights are *not* a subset of its parent's.
    pub fn contains(&self, other: &CapabilityRights) -> bool {
        self.operations.contains(other.operations) && self.timeout_ms >= other.timeout_ms
    }

    /// Return the largest rights that is ≤ `self` AND ≤ `other` —
    /// i.e. the intersection. Used when `restrict` silently clamps a
    /// caller that asks for more than it has.
    pub fn intersect(&self, other: &CapabilityRights) -> CapabilityRights {
        CapabilityRights {
            operations: self.operations & other.operations,
            timeout_ms: self.timeout_ms.min(other.timeout_ms),
        }
    }
}

// Phase 17: `parse_operation` is deleted. The `Rights` type is
// role-typed; Protocol operations are defined by the Protocol
// itself, not by a global verb table. There is no global verb
// vocabulary to parse against.
