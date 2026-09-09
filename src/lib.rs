//! Odyssey library — the capability kernel + plugins.
//!
//! The single binary (`cargo run -- boot`) drives a 7-phase boot
//! that mints typed `Capability<R>` tokens from every plugin manifest
//! in `src/plugins/`, exercises them through the HTTP bridge, and
//! exposes the layout via the `CapabilityGraph`.

pub mod capability;
pub mod host;
pub mod plugins;