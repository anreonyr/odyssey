//! Capability layer — typed (`Capability<R>`) and erased (`AnyCapability`).

pub mod erased;
pub mod typed;

pub use erased::AnyCapability;
pub use typed::Capability;
