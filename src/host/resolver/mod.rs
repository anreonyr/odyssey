//! Phase 8: re-export shim.
//!
//! The resolver content physically lives in
//! `crate::personality::composition::resolve`. This module
//! re-exports it at the old `host::resolver::Foo` path so the
//! existing call sites in `runtime/` continue to work during
//! the migration.

pub use crate::personality::composition::resolve::{
    resolve, Reachable, ResolveError, ResolvedBinding, ResolvedPlan,
};
