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

/// Rights attached to a capability. Supplied when deriving a child via
/// `grant`, `transfer`, or `restrict`. Two dimensions:
///
/// - `operations` — what the holder may *do* (`READ`/`WRITE`/...).
///   Attenuation (`restrict`) enforces `child ⊆ parent`.
/// - `timeout_ms` — wall-clock budget per call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilityRights {
    pub operations: OperationRights,
    pub timeout_ms: u32,
}

impl Default for CapabilityRights {
    fn default() -> Self {
        Self {
            operations: OperationRights::ALL,
            timeout_ms: 5000,
        }
    }
}

impl CapabilityRights {
    /// Construct an "all authority, default timeout" rights bag — what
    /// the host uses when minting a root capability.
    pub fn root(timeout_ms: u32) -> Self {
        Self {
            operations: OperationRights::ALL,
            timeout_ms,
        }
    }

    #[allow(dead_code)]
    pub fn with_timeout(mut self, ms: u32) -> Self {
        self.timeout_ms = ms;
        self
    }

    #[allow(dead_code)]
    pub fn with_operations(mut self, ops: OperationRights) -> Self {
        self.operations = ops;
        self
    }

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
