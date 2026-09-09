//! Slow plugin — sleeps longer than its declared budget to exercise
//! the timeout enforcement in `Capability::invoke`.

mod handler;
pub use handler::*;
