//! Identifiers — `CapabilityId`, `SlotId`, `PluginId`.
//!
//! Pure value types. No allocation, no locking, no I/O. Owned by
//! the kernel layer because all three are identities that flow
//! through every authority boundary.
//!
//! Phase 5 owner decision: `PluginId` migrates here from
//! `host::manifest` (formerly `kernel::manifest::PluginId`). The
//! kernel layer is the home for "things that are an identity".

use std::fmt;
use std::num::NonZeroU64;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// CapabilityId
// ---------------------------------------------------------------------------

/// Unforgeable capability identifier (the `cap:0`, `cap:1` namespace).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityId(pub u64);

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cap:{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// SlotId
// ---------------------------------------------------------------------------

/// Stable position in a `CapabilitySpace`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SlotId(NonZeroU64);

impl SlotId {
    /// Construct a SlotId from a raw u64. Panics if `raw` is 0
    /// (SlotIds are non-zero by construction; the CSpace allocator
    /// starts at 1).
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
// PluginId
// ---------------------------------------------------------------------------

/// Plugin identity: `(name, version)`. Carried inside `CapabilityMeta`
/// so the HTTP bridge and resolver can answer "who minted this?".
///
/// Phase 5: lives here next to `CapabilityId` / `SlotId`. Manifest
/// parsing/validation moves to `host::manifest`; the type itself
/// is a pure identity and belongs in the kernel.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct PluginId {
    pub name: String,
    pub version: String,
}
