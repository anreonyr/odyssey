//! Echo plugin — `EchoResource: Resource`. Sync only; `open` falls through
//! to the trait's default "not a streaming capability" implementation.

mod handler;
pub use handler::*;
