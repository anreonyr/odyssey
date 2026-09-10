//! γ — Observability (Phase 2 P6 + protocol/authority metadata).
//!
//! Tests that prove the runtime can *see* the capability layout it
//! has minted:
//!
//! - **γ.1 — Capability graph** ([`p6_graph`]) (P6):
//!   `CapabilityGraph::from(&cspace)` and `children_of(slot)` expose
//!   the parent→child attenuation tree to runtime code.
//! - **γ.2 — Protocol metadata roundtrips through TOML**
//!   ([`contract_toml`]): the `[exposes.protocol]` block in a
//!   manifest parses into `Protocol`. Phase 3 P3.2 split the
//!   old `[exposes.contract]` block into `[exposes.protocol]`
//!   (wire metadata) and `[[exposes.authority.actions]]`
//!   (authority vocabulary).
//! - **γ.3 — Protocol/authority forwarded by the factory**
//!   ([`contract_factory`]): a `CapabilityDecl` with non-empty
//!   `protocol` and `authority` reaches `slot.meta().protocol` and
//!   `slot.meta().authority`.
//! - **γ.4 — Defaults when manifest omits both**
//!   ([`contract_default`]): missing `protocol` and `authority`
//!   both fall back to empty structs.
//! - **γ.5 — End-to-end chain** ([`contract_end_to_end`]): TOML →
//!   `PluginManifest` → `CapabilityDecl` → factory.mint →
//!   `CapabilityMeta` → `slot_meta(slot)`. The protocol/authority
//!   on disk is what's observable on the slot.
//! - **γ.6 — Authority action table** ([`contract_actions`]): the
//!   `[[exposes.authority.actions]]` blocks deserialise into
//!   `AuthorityContract.actions`, the load-bearing input for
//!   type-agnostic dispatch.

mod contract_actions;
mod contract_default;
mod contract_end_to_end;
mod contract_factory;
mod contract_toml;
mod p6_graph;

#[path = "../common/mod.rs"]
mod common;