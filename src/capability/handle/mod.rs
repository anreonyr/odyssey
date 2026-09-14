//! Capability / handle — typed and erased capability handles.
//!
//! `Slot<R>` is what plugin bodies hold (typed, unforgeable).
//! `Capability<R>` is what the cspace stores (typed, internal).
//! `AnyCapability` is the erased view the cspace uses to keep
//! heterogeneous caps in one HashMap.

pub mod cap;
pub mod slot;
