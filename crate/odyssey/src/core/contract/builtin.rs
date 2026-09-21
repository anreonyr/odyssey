//! `BuiltinManifest` trait — the only thing a workspace-member
//! builtin exposes to the personality orchestrator.
//!
//! Phase 8 design note: the original plan had a generic
//! `Builtin` trait with an associated `Resource` type so that
//! builtins could be held as `Arc<dyn Builtin>` in a list.
//! That design hit a wall: the kernel's `CapabilityFactory::mint`
//! is generic over `R`, and a trait object can't dispatch into a
//! generic call. Adding the erasure path (factory.mint takes
//! `Arc<dyn Resource>`, kernel stores `Box<dyn Resource>`) was
//! deemed too invasive for the Phase 8 cleanup window.
//!
//! Pragmatic resolution: builtins stay concrete types in
//! `builtins/` (`EchoBuiltin`, `ReverseBuiltin`, `DatabaseBuiltin`).
//! The personality orchestrator takes the three concrete builtins
//! as separate parameters. No trait object, no dispatch table —
//! just three function calls. Adding a fourth builtin means
//! adding a fourth parameter; that's the cost we pay for
//! preserving the typed `Capability<R>` invariant.
//!
//! This trait exists because every builtin still needs to
//! expose a `manifest()` so the resolver can compute mint
//! order. The manifest is the only thing personality needs from
//! a homogeneous list view.

use crate::core::contract::resource::Resource;
use crate::core::manifest::manifest::PluginManifest;

/// Capability manifest returned by a builtin. All builtins
/// implement this so the personality orchestrator can collect
/// manifests into a homogeneous list.
pub trait BuiltinManifest: Send + Sync {
    /// DI Phase 21: the resource type this builtin hands to the
    /// kernel at mint. Type-only associated type (no method
    /// bodies); the standard workaround for "need R but cannot
    /// dispatch via vtable" — `CapabilityFactory::mint<R>` is
    /// generic over `R`, and a trait object can't dispatch into
    /// a generic call. Each concrete builtin declares
    /// `type Resource = XResource;`.
    type Resource: Resource;
    fn manifest(&self) -> PluginManifest;
}
