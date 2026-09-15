//! Metadata vocabulary — `CapabilityMeta` (the capability
//! description surfaced to dispatch + the HTTP bridge) plus
//! `CapabilityChunk` (the stream item type).
//!
//! Phase 9 cleanup: `AuthorityContract` and `Protocol` were
//! removed — both were write-only metadata (constructed by
//! the manifest builder, propagated into `CapabilityMeta`,
//! never read). See `meta.rs` for the rationale.

pub mod chunk;
pub mod meta;
