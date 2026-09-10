//! δ — Rate-limit authority (Phase 2 quota extension).
//!
//! - **δ.1 — Quota blocks at the per-minute limit** ([`quota_basic`]):
//!   a `Capability` minted with `calls_per_minute = N` denies the
//!   (N+1)-th call.
//! - **δ.2 — Quota is shared across `restrict`** ([`quota_shared`]):
//!   derived caps draw from the parent's rate-limit bucket, so the
//!   parent's calls debit the same counter the child observes.

mod quota_basic;
mod quota_clock_eviction;
mod quota_shared;

#[path = "../common/mod.rs"]
mod common;