//! ζ — Phase 3.4 capability-native agent tests.
//!
//! These tests verify that the RuleAgent's reachable set
//! derives from the **resolved binding table**, not from
//! externally-supplied slot lists. The same `AgentResource`
//! binary, fed two different `ResolvedPlan`s, must produce
//! two different observable worlds.
//!
//! - **ζ.1** ([`p3_4_capability_native`]):
//!   - ζ.1.a — same agent binary, manifest declares
//!     `requires: counter` → reachable = `[counter]`. Calling
//!     with `target="counter"` succeeds; `target="echo"` is
//!     rejected as unreachable.
//!   - ζ.1.b — same agent binary, manifest declares
//!     `requires: []` → reachable = `[]`. Every dispatch
//!     rejected.
//!   - ζ.1.c — same agent binary, manifest declares
//!     `requires: counter, echo` → reachable =
//!     `[counter, echo]`. Both targets accepted (if the
//!     providers exist in cspace).
//! - **ζ.2** ([`p3_4_capability_native`]): two agents with
//!   different binding tables dispatch on the same `target`
//!   and reach different caps — proving the binding's
//!   `capability` field (not just `handle`) drives cspace
//!   resolution.
//!
//! ## Phase 3.6 — Runtime Lifetime
//!
//! - **ζ.4** ([`p3_6_lifetime`]): teardown order is reverse of
//!   mint order; cspace ends empty.
//! - **ζ.5** ([`p3_6_lifetime`]): provider revoke invalidates
//!   consumer's reachable entry — the P3.4 ↔ P3.6 handshake.
//! - **ζ.6** ([`p3_6_lifetime`]): `revoke_tree` propagates
//!   through every derived cap in the chain.
//! - **ζ.7** ([`p3_6_lifetime`]): consumer teardown doesn't
//!   touch provider.
//!
//! ## Phase 3.2 — Protocol as Metadata
//!
//! - **ζ.8** ([`p3_2_protocol`]): same binary, with/without
//!   protocol → same dispatch (protocol is metadata, not a
//!   contract).
//! - **ζ.9** ([`p3_2_protocol`]): protocol is queryable from
//!   `cap.meta().protocol`; wire metadata survives mint.
//! - **ζ.10** ([`p3_2_protocol`]): `AuthorityContract` is the
//!   agent's only authority source; `meta.authority.operation_for`
//!   drives the action → bit translation.
//! - **ζ.11** ([`p3_2_protocol`]): `ManifestBuilder::protocol(...)`
//!   roundtrips through the same fields as a hand-built struct.
//!
//! ## Phase 3.7 — Graph Events
//!
//! - **ζ.12** ([`p3_7_events`]): single mint fires one `Minted`
//!   event with the right fields.
//! - **ζ.13** ([`p3_7_events`]): `revoke_tree` fires `Revoked`
//!   per slot + one `RevokeTree` with total.
//! - **ζ.14** ([`p3_7_events`]): `grant`/`restrict`/`transfer`
//!   fire `Derived` events with distinct `DeriveKind`s.
//! - **ζ.15** ([`p3_7_events`]): multi-subscriber semantics:
//!   every subscriber gets every event.
//! - **ζ.16** ([`p3_7_events`]): full lifecycle event sequence
//!   matches the expected boot shape (mint order → activate →
//!   shutdown reverse order → completed).
//! - **ζ.21** ([`p3_7_events`]): multi-slot plugin shutdown
//!   emits per-slot Revoked + RevokeTree events (1 + 2N events
//!   for a plugin with N [[exposes]] blocks), not the single-
//!   RevokeTree shape a one-per-plugin reader might assume.
//!
//! ## Phase 3.3 — Real AI Resources
//!
//! - **ζ.17** ([`p3_3_real_models`]): `MockModel` preserves
//!   the historical canned behaviour for back-compat.
//! - **ζ.18** ([`p3_3_real_models`]): `MarkovModel` produces
//!   non-trivial, prompt-sensitive, deterministic output.
//! - **ζ.19** ([`p3_3_real_models`]): boot selects `Markov`
//!   by default and `Mock` when `GENERATOR_MODEL=mock`.
//! - **ζ.20** ([`p3_3_real_models`]): runtime handler streams
//!   tokens in order; both models end with `[end]` + `Done`.

mod p3_2_protocol;
mod p3_3_real_models;
mod p3_4_capability_native;
mod p3_6_lifetime;
mod p3_7_events;
mod p4_4_5_agent_env;
mod p4_6_delegation;
mod p4_7_composition;
mod p4_8_revocation;
mod p4_8_stream_cancel;

#[path = "../common/mod.rs"]
mod common;