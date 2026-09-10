//! Phase 5 — concurrency test gate.
//!
//! These tests reproduce the three CRITICAL/MAJOR defects identified
//! in the Phase 4 review-loop and Pin 5 council memo. They are
//! opt-in (`cargo test --features loom-tests`) so the default build
//! does not pay for the loom runtime. Each test MUST FAIL on the
//! pre-Phase-5 code, then MUST PASS once the fix lands.
//!
//! ## D2 — install_derived dead-parent race
//!
//! `revoke_tree` first reads `parents` (under `parents.read()`) to
//! collect children, then drops the guard, then calls
//! `revoke_with_sweep` which acquires `parents.write()` + `slots.write()`
//! + `names.write()` and removes the slot.
//!
//! If `install_derived` wins the `parents.write()` lock AFTER
//! `revoke_with_sweep` releases it, the freshly-installed cap has a
//! parent pointer to a slot id that no longer exists in any map. The
//! cap is dispatchable (it lives in `slots` + `names`), with
//! `revoked == false`, and is unreachable from any future
//! `revoke_tree` (its parent is dead). The kernel-level guarantee
//! "`revoke_tree` propagates to every descendant" is violated.
//!
//! ## D3 — share_quota_with allocates fresh wall-clock counter
//!
//! `share_quota_with` clones `quota_state` (correct) but allocates a
//! fresh `Arc<AtomicU64>` for `wall_clock_total_ms`. For a root cap
//! that has accumulated `wall_clock_total_ms = 1000` and a child via
//! `restrict` that runs 100 more calls, the child's counter reads
//! `100`, not `1100`. The HTTP bridge and graph dump therefore
//! misreport per-subtree usage.

mod d2_install_derived_dead_parent;
mod d3_wall_clock_subtree_accumulates;
mod d5_dead_quota_fields;
