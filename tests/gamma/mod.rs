//! γ — Observability (Phase 2 P6 + contract metadata).
//!
//! Tests that prove the runtime can *see* the capability layout it
//! has minted:
//!
//! - **γ.1 — Capability graph** ([`p6_graph`]) (P6):
//!   `CapabilityGraph::from(&cspace)` and `children_of(slot)` expose
//!   the parent→child attenuation tree to runtime code.
//! - **γ.2 — Contract metadata roundtrips through TOML**
//!   ([`contract_toml`]): the `[exposes.contract]` block in a
//!   manifest parses into `CapabilityContract`.
//! - **γ.3 — Contract forwarded by the factory** ([`contract_factory`]):
//!   a `CapabilityDecl` with a non-empty contract reaches
//!   `slot.meta().contract`.
//! - **γ.4 — Contract defaults when manifest omits it**
//!   ([`contract_default`]): missing `[exposes.contract]` falls back
//!   to `CapabilityContract::default()`.
//! - **γ.5 — End-to-end chain** ([`contract_end_to_end`]): TOML →
//!   `PluginManifest` → `CapabilityDecl` → factory.mint →
//!   `CapabilityMeta` → `slot_meta(slot)`. The contract that lives
//!   on disk is the contract that's observable on the slot.

mod contract_default;
mod contract_end_to_end;
mod contract_factory;
mod contract_toml;
mod p6_graph;

#[path = "../common/mod.rs"]
mod common;