//! Basic capability types — kind discriminator, identifiers, metadata,
//! rights, budget, chunks, and the error enum. No behaviour; just data.

use std::fmt;
use std::num::NonZeroU64;

use bitflags::bitflags;
use serde_json::Value;

use crate::host::manifest::PluginId;

// ---------------------------------------------------------------------------
// Kinds
// ---------------------------------------------------------------------------

/// Runtime distinction between sync and streaming capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapKind {
    Sync,
    Stream,
}

// ---------------------------------------------------------------------------
// Identifier
// ---------------------------------------------------------------------------

/// Unforgeable capability identifier (the `cap:0`, `cap:1` namespace).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityId(pub u64);

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cap:{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Slot identifier
// ---------------------------------------------------------------------------

/// Stable position in a `CapabilitySpace`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotId(NonZeroU64);

impl SlotId {
    /// Construct a SlotId from a raw u64. Panics if `raw` is 0 (SlotIds
    /// are non-zero by construction; the CSpace allocator starts at 1).
    pub fn new(raw: u64) -> Self {
        Self(NonZeroU64::new(raw).expect("SlotId::new called with 0"))
    }

    pub fn raw(&self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for SlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "slot:{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

/// Static description of a capability. Carried inside every token;
/// surfaced to the HTTP bridge for enumeration.
#[derive(Clone, Debug)]
pub struct CapabilityMeta {
    pub id: CapabilityId,
    pub name: String,
    pub plugin: PluginId,
    pub in_type: String,
    pub out_type: String,
    pub streaming: bool,
    pub timeout_ms: u32,
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

/// Single-call wall-clock budget. Held inside every token via `Arc`,
/// which lets multiple clones enforce the same limit consistently.
#[derive(Clone, Debug)]
pub struct CapabilityBudget {
    pub timeout_ms: u32,
}

impl CapabilityBudget {
    pub fn new(timeout_ms: u32) -> Self {
        Self { timeout_ms }
    }
}

// ---------------------------------------------------------------------------
// Rights
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Stream chunks
// ---------------------------------------------------------------------------

/// One chunk in a streaming capability response.
#[derive(Debug)]
pub enum CapabilityChunk<T = Value> {
    Item(T),
    Done,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by capability operations on the CSpace.
#[derive(Debug)]
pub enum CapabilityError {
    AlreadyExists(String),
    /// The slot was empty or revoked when an operation tried to read it.
    SlotEmpty(SlotId),
    /// `restrict` (or `grant`) asked for rights the parent does not have.
    /// This is the "you cannot amplify authority" invariant.
    AttenuationViolation {
        from: SlotId,
        requested: OperationRights,
        held: OperationRights,
    },
    /// The capability was invoked with an operation bit it does not hold.
    OperationDenied {
        name: String,
        requested: OperationRights,
        held: OperationRights,
    },
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(n) => write!(f, "capability already installed: {n}"),
            Self::SlotEmpty(s) => write!(f, "slot {} empty or revoked", s.raw()),
            Self::AttenuationViolation { from, requested, held } => write!(
                f,
                "attenuation violation at slot {}: requested {:?} not subset of {:?}",
                from.raw(),
                requested,
                held
            ),
            Self::OperationDenied { name, requested, held } => write!(
                f,
                "operation denied for capability \"{name}\": requested {:?} not in {:?}",
                requested,
                held
            ),
        }
    }
}

impl std::error::Error for CapabilityError {}

impl From<CapabilityError> for cordis::Error {
    fn from(e: CapabilityError) -> Self {
        cordis::Error::msg(e.to_string())
    }
}
