//! Host services — factory, manifest, pipeline, http bridge, and the
//! 7-phase boot orchestration. This module is the kernel-side logic of
//! odyssey; the plugin bodies live in `crate::plugins`.

pub mod boot;
pub mod factory;
pub mod http_bridge;
pub mod manifest;
pub mod pipeline;
pub mod registry;