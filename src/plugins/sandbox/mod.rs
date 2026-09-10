//! Plugin manifest — Phase 3 single-source-of-truth. See
//! [`manifest`] for the typed `PluginManifest` returned to the
//! runtime.

mod handler;
pub use handler::*;
mod manifest;
pub use manifest::manifest;
