//! Plugin manifest — Phase 3 single-source-of-truth. See
//! [`manifest`] for the typed `PluginManifest` returned to the
//! runtime.

mod handler;
pub use handler::*;
pub mod model;
pub use model::{MarkovModel, MockModel, Model, ModelKind};

mod manifest;
pub use manifest::manifest;