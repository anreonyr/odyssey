//! Kernel-side minting and composition.
//!
//! These are the operations that turn a `PluginManifest` into a
//! populated `CapabilitySpace`:
//!
//! - [`factory`] — `CapabilityFactory` mints typed `Capability<R>`
//!   into the cspace and tracks every minted `CapabilityMeta`.
//! - [`manifest`] — the data contract that plugin authors write.
//! - [`registry`] — duplicate-name detection across manifests.
//! - [`pipeline`] — composes sync caps into a stage list.
//!
//! Boot orchestration and the HTTP bridge live in
//! [`crate::boot`]. The capability model (CSpace, slots, rights,
//! resource trait) lives in [`crate::capability`].

pub mod factory;
pub mod manifest;
pub mod mint;
pub mod pipeline;
pub mod registry;