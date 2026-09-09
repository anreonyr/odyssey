//! α — Authority lifecycle (Phase 1 foundation).
//!
//! The six properties the capability model must satisfy:
//!
//! - **α.1 — Authority** ([`authority`]) — a `Capability<R>` consulted
//!   with the right bit runs the handler; consulted without it is
//!   denied.
//! - **α.2 — Delegation** ([`delegation`]) — a plugin
//!   (`BrokerResource`) holding a capability can mint a derived slot
//!   via `cspace.restrict`.
//! - **α.3 — Restrict / attenuation** ([`attenuation`]) —
//!   `cspace.restrict(child ⊇ parent)` returns
//!   `AttenuationViolation`.
//! - **α.4 — Revocation** ([`revocation`]) — `cspace.revoke(slot_id)`
//!   makes subsequent `Slot::invoke` calls fail; the slot reference
//!   itself stays valid (no panic).
//! - **α.5 — Budget** ([`budget`]) — a `Slow` resource exceeding its
//!   budget returns a timeout error; a generous budget succeeds.
//! - **α.6 — Composition** ([`composition`]) — slot-bound pipeline
//!   stages observe revocation mid-run via
//!   `PipelineError::SlotRevoked`.
//!
//! Plus two bonus tests for the operations `grant` ([`grant`]) and
//! `transfer` ([`transfer`]) that fall out of α.2.
//!
//! Every test boots its own CSpace + factory via `crate::common::boot()`
//! and uses the Counter, Slow, and Echo resources from the plugin tree
//! so the tests exercise the same code path as the runtime.

mod authority;
mod attenuation;
mod budget;
mod composition;
mod delegation;
mod grant;
mod revocation;
mod transfer;

// `common/` lives at tests/common/, not tests/alpha/common/, so
// we point the module at its real path explicitly.
#[path = "../common/mod.rs"]
mod common;