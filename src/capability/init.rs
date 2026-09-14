//! Capability kernel — single entry point for callers.
//!
//! The only function in `capability/` that personality calls.
//! Returns an opaque `Box<dyn ...>` (the trait abstraction lives
//! in `core/contract/factory.rs` in a later commit; for now we
//! return a concrete `KernelFactory`).
//!
//! The kernel layer is the only place that creates
//! `CapabilitySpace`. Personality uses the factory to mint
//! capabilities into the space, then operates on the space via
//! lookup / enumerate / revoke.

use std::sync::Arc;

use crate::capability::enforce::space::CapabilitySpace;
use crate::core::clock::clock::Clock;

/// Concrete factory — wraps a `CapabilitySpace` and threads the
/// `Clock` into every minted capability.
pub struct KernelFactory {
    space: CapabilitySpace,
    clock: Arc<dyn Clock>,
}

impl KernelFactory {
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            space: CapabilitySpace::new(),
            clock,
        }
    }

    pub fn space(&self) -> &CapabilitySpace {
        &self.space
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }
}

/// The single entry point. Personality calls this to obtain
/// a kernel-backed factory without depending on any concrete
/// `capability::*` type — the return type is opaque.
pub fn new_kernel(clock: Arc<dyn Clock>) -> KernelFactory {
    KernelFactory::new(clock)
}
