//! Basic capability types — kind discriminator, identifiers, metadata,
//! rights, budget, chunks, and the error enum. No behaviour; just data.

use std::fmt;
use std::num::NonZeroU64;

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

/// Rights attached to a capability. Supplied when deriving a child via
/// `grant`, `transfer`, or `restrict`. Currently a single dimension
/// (`timeout_ms`); extensible to rights bits later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilityRights {
    pub timeout_ms: u32,
}

impl Default for CapabilityRights {
    fn default() -> Self {
        Self { timeout_ms: 5000 }
    }
}

impl CapabilityRights {
    #[allow(dead_code)]
    pub fn with_timeout(mut self, ms: u32) -> Self {
        self.timeout_ms = ms;
        self
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
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(n) => write!(f, "capability already installed: {n}"),
            Self::SlotEmpty(s) => write!(f, "slot {} empty or revoked", s.raw()),
        }
    }
}

impl std::error::Error for CapabilityError {}

impl From<CapabilityError> for cordis::Error {
    fn from(e: CapabilityError) -> Self {
        cordis::Error::msg(e.to_string())
    }
}
