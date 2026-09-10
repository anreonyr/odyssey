//! Embedder plugin — `embed(text) → Vec<f32>`. Sync only.

mod handler;
pub use handler::{embedder_plugin, handler, EmbedderResource};

pub(crate) mod manifest;
pub use manifest::manifest;
