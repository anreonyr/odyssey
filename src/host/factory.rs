//! Phase 8: re-export shim.
//!
//! The factory + mint-time helpers physically live in
//! `crate::personality::lifecycle::mint`. This module
//! re-exports them at the old `host::factory::Foo` path so the
//! existing call sites in `runtime/` continue to work during
//! the migration.

pub use crate::personality::lifecycle::mint::{
    meta_from_decl, namespace_for, CapabilityFactory,
};
