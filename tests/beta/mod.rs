//! β — Multi-actor composition (Phase 2 extension).
//!
//! Tests that need *more than one capability* interacting:
//!
//! - **β.1 — Multi-hop delegation** ([`p2_multihop`]) (extends P2):
//!   three brokers chained through `cspace.restrict` preserve the
//!   `rights(c) ⊆ rights(b) ⊆ rights(a) ⊆ rights(root)` invariant.
//! - **β.2 — Multi-hop revocation** ([`p3_revoke_tree`]) (extends
//!   P3/P4): revoking an intermediate hop (`revoke_tree`) severs
//!   every descendant.
//! - **β.3 — Capability-controlled communication** ([`p4_channel`])
//!   (P4): a `Capability<Channel>` is the producer's authority to
//!   send; revoking the channel slot severs the connection.
//! - **β.4 — Same program, different authority** ([`p5_agent`])
//!   (P5): two `RuleAgent` instances built from identical code but
//!   different capability environments produce different observable
//!   behaviour.

mod p2_multihop;
mod p3_revoke_tree;
mod p4_channel;
mod p5_agent;

// `common/` lives at tests/common/, not tests/beta/common/, so
// we point the module at its real path explicitly.
#[path = "../common/mod.rs"]
mod common;