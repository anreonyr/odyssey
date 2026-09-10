//! Capability errors — typed variants for every kernel-level failure.
//!
//! Phase 5 M4 fix: the typed variants replace the Phase 4 `String`
//! errors returned by `Capability::invoke_op`. Callers (pipeline,
//! HTTP bridge, agent) pattern-match on the variant instead of doing
//! substring matching on error messages.

use std::fmt;

use crate::kernel::ids::SlotId;
use crate::kernel::quota::QuotaKind;
use crate::kernel::rights::OperationRights;

/// Errors returned by capability operations on the CSpace.
#[derive(Debug)]
pub enum CapabilityError {
    /// A capability with this name was already installed in the slot.
    AlreadyExists(String),
    /// The slot was empty or revoked when an operation tried to read it.
    /// Phase 5 D2: also returned by `install_derived` when the parent
    /// id no longer exists in `parents` (race-condition guard).
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
    /// Phase 5 M4: the requested call would mix sync / stream kinds.
    /// Phase 4 used a generic `String` error; typed variant now.
    KindMismatch { name: String, expected: &'static str, got: &'static str },
    /// The rate-limit quota exhausted.
    QuotaExceeded { name: String, kind: QuotaKind },
    /// Domain error from the handler implementation. Phase 5: the
    /// handler returns `Result<Value, String>`; we wrap that here
    /// so `invoke_op`'s outer signature is typed `CapabilityError`
    /// (M4). Pipeline and HTTP bridge still see the inner String
    /// via `Display`, but the kernel-level invariants are now
    /// distinguishable by variant.
    Handler { name: String, message: String },
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
            Self::KindMismatch { name, expected, got } => write!(
                f,
                "capability \"{name}\": expected {expected} capability, called as {got}"
            ),
            Self::QuotaExceeded { name, kind } => {
                write!(f, "quota exhausted for capability \"{name}\": {kind}")
            }
            Self::Handler { name, message } => {
                write!(f, "handler error for capability \"{name}\": {message}")
            }
        }
    }
}

impl std::error::Error for CapabilityError {}

impl From<CapabilityError> for cordis::Error {
    fn from(e: CapabilityError) -> Self {
        cordis::Error::msg(e.to_string())
    }
}
