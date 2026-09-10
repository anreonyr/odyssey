//! HTTP plugin — provides `http_request` capability (mock
//! backend; real backend is Phase 5 work).

mod handler;
pub use handler::{handler, http_plugin, HttpResource};

pub(crate) mod manifest;
pub use manifest::manifest;
