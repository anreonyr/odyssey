//! Operation rights — the bitflag carried inside every `Capability<R>`
//! and every `CapabilityRights` (the attenuation shape).
//!
//! Phase 5: split from `capability::types`. The `parse_operation` helper
//! used to live in `plugins/agent/handler.rs` (Phase 4); it moves here
//! as the single home for the action-verb → bit mapping.

use bitflags::bitflags;

bitflags! {
    /// Per-call operations a capability permits. The kernel (CSpace) is
    /// the only thing that *creates* these; `Capability::invoke` consults
    /// the held rights at every call to reject an operation that was
    /// dropped by `restrict`.
    ///
    /// This is Phase 1's first real "authority": a bit you can subtract,
    /// bit you cannot expand, bit the resource can introspect.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct OperationRights: u32 {
        /// Read / observe the resource's state.
        const READ      = 1 << 0;
        /// Mutate the resource's state.
        const WRITE     = 1 << 1;
        /// Invoke an effect (start a computation, run an actor, ...).
        const EXECUTE   = 1 << 2;
        /// Lifecycle authority — re-grant, restrict, revoke.
        const ADMIN     = 1 << 3;
        /// Convenience: every bit set. Used when minting the root cap.
        const ALL       = Self::READ.bits() | Self::WRITE.bits()
                        | Self::EXECUTE.bits() | Self::ADMIN.bits();
    }
}

impl Default for OperationRights {
    fn default() -> Self {
        Self::ALL
    }
}

// -----------------------------------------------------------------------
// Slice 1 of the INVOKE / ASSIGN / REVOKE redesign.
// -----------------------------------------------------------------------
//
// The new `Rights` is the kernel's first-class authority type.
// `OperationRights` is preserved for the migration window only;
// new code must use `Rights`. Slice 5 deletes `OperationRights`.
//
// Capability = Plugin-to-Plugin Authority.
// The bit semantics are role-typed, not operation-typed:
//   INVOKE — use the capability (call into the Protocol).
//   ASSIGN — propagate the capability to other plugins.
//   REVOKE — retract the capability (scoped to edges the
//            holder created; transitive over descendants).
// Protocol operations (query / insert / start / ...) are NOT encoded
// in `Rights`. They live in the Protocol layer.

bitflags! {
    /// Per-edge authority bits. The kernel (CapabilitySpace) is the
    /// only thing that *creates* these; `Capability::invoke` consults
    /// the held rights at every call to reject an operation that was
    /// dropped by `restrict`.
    ///
    /// Slice 1 of the INVOKE / ASSIGN / REVOKE redesign. Replaces
    /// `OperationRights`. The three bits are the orthogonal axes
    /// of a Plugin-to-Plugin Authority edge: traversal, propagation,
    /// retraction.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct Rights: u32 {
        /// Use the capability: invoke the underlying Protocol.
        /// Replaces `READ | WRITE | EXECUTE` from `OperationRights`
        /// — those were operation-typed verbs; this is a single
        /// role-typed authority bit.
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

    /// Conservative collapse from the legacy `OperationRights`.
    /// Used by the migration window's `From` impl; deleted at
    /// slice 5 alongside `OperationRights` itself.
    pub fn from_legacy(legacy: OperationRights) -> Rights {
        let mut r = Rights::empty();
        if legacy.contains(OperationRights::READ) {
            r |= Rights::INVOKE;
        }
        if legacy.contains(OperationRights::WRITE) {
            r |= Rights::INVOKE;
        }
        if legacy.contains(OperationRights::EXECUTE) {
            r |= Rights::INVOKE;
        }
        if legacy.contains(OperationRights::ADMIN) {
            r |= Rights::REVOKE;
        }
        r
    }
}

impl From<OperationRights> for Rights {
    fn from(o: OperationRights) -> Self {
        Rights::from_legacy(o)
    }
}

impl From<Rights> for OperationRights {
    fn from(r: Rights) -> Self {
        let mut o = OperationRights::empty();
        if r.contains(Rights::INVOKE) {
            o |= OperationRights::READ | OperationRights::WRITE | OperationRights::EXECUTE;
        }
        if r.contains(Rights::ASSIGN) {
            o |= OperationRights::ADMIN; // closest legacy analog
        }
        if r.contains(Rights::REVOKE) {
            o |= OperationRights::ADMIN;
        }
        o
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

/// Translate the contract-level action verb (`"READ"` / `"WRITE"` /
/// `"EXECUTE"` / `"ADMIN"`) into the corresponding `OperationRights`
/// bit. Returns `None` for unknown verbs.
///
/// Phase 5: this used to live in `plugins/agent/handler.rs`. The agent
/// is one consumer, but the vocabulary is global, so the parser
/// belongs with the rights type.
pub fn parse_operation(verb: &str) -> Option<OperationRights> {
    match verb {
        "READ" => Some(OperationRights::READ),
        "WRITE" => Some(OperationRights::WRITE),
        "EXECUTE" => Some(OperationRights::EXECUTE),
        "ADMIN" => Some(OperationRights::ADMIN),
        _ => None,
    }
}
