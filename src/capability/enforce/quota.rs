//! Quota runtime state — `QuotaState` (accounting) +
//! `CapabilityBudget` (per-call ceiling + wall-clock counter).
//!
//! Phase 8 split: the value types (`QuotaSpec`, `QuotaKind`,
//! `QuotaSnapshot`) moved to `core::quota` because they're pure
//! data. What stays here needs locks / `Arc` / runtime state,
//! and depends on `Clock` from `core::clock::clock` — so this
//! file lives in `capability` (the kernel layer).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use crate::core::clock::clock::{Clock, SystemClock};
use crate::core::quota::quota::{QuotaKind, QuotaSnapshot, QuotaSpec};

// ---------------------------------------------------------------------------
// QuotaState — runtime accounting
// ---------------------------------------------------------------------------

/// Quota accounting state. Sliding-window, held behind `Arc` so
/// derived caps share the parent's bucket.
///
/// Phase 5:
///   - `tokens_per_minute` / `bytes_per_minute` removed (D5).
///   - `try_tokens` / `try_bytes` removed (D5).
///   - `Instant::now()` replaced with `Clock::now()` injection.
///   - `QuotaSpec` is now `{ calls_per_minute: u32 }` only.
pub struct QuotaState {
    spec: QuotaSpec,
    clock: Arc<dyn Clock>,
    inner: RwLock<QuotaStateInner>,
}

impl std::fmt::Debug for QuotaState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuotaState")
            .field("spec", &self.spec)
            .field("clock", &"<dyn Clock>")
            .finish()
    }
}

#[derive(Debug, Default)]
struct QuotaStateInner {
    call_stamps: Vec<Instant>,
    last_exhausted: Option<QuotaKind>,
}

impl QuotaState {
    /// Construct an empty quota state for a given spec, using
    /// the provided `Clock`. Production uses `SystemClock`;
    /// tests inject `MockClock`.
    pub fn new(spec: QuotaSpec, clock: Arc<dyn Clock>) -> Self {
        Self {
            spec,
            clock,
            inner: RwLock::new(QuotaStateInner::default()),
        }
    }

    pub fn spec(&self) -> QuotaSpec {
        self.spec
    }

    /// Try to consume one call. Returns `Err(QuotaKind)` if
    /// the per-minute limit would be exceeded.
    pub fn try_call(&self) -> Result<(), QuotaKind> {
        let now = self.clock.now();
        let mut g = self.inner.write().expect("quota poisoned");
        Self::evict(&mut g.call_stamps, now);
        let limit = self.spec.calls_per_minute;
        if limit == 0 {
            g.call_stamps.push(now);
            g.last_exhausted = None;
            return Ok(());
        }
        if g.call_stamps.len() as u32 >= limit {
            g.last_exhausted = Some(QuotaKind::Calls);
            return Err(QuotaKind::Calls);
        }
        g.call_stamps.push(now);
        g.last_exhausted = None;
        Ok(())
    }

    /// Snapshot used-this-minute for diagnostics / HTTP bridge.
    pub fn snapshot(&self) -> QuotaSnapshot {
        let g = self.inner.read().expect("quota poisoned");
        QuotaSnapshot {
            calls_used: g.call_stamps.len() as u32,
            last_exhausted: g.last_exhausted,
        }
    }

    fn evict(stamps: &mut Vec<Instant>, now: Instant) {
        let cutoff = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);
        stamps.retain(|t| *t >= cutoff);
    }
}

// ---------------------------------------------------------------------------
// CapabilityBudget — per-call wall-clock + quota
// ---------------------------------------------------------------------------

/// Single-call wall-clock budget. Held inside every token via
/// `Arc`, which lets multiple clones enforce the same limit
/// consistently.
///
/// Phase 5 D3 fix: `share_quota_with` clones the parent's
/// `Arc<AtomicU64>` instead of allocating a fresh counter.
/// Derived caps therefore report subtree-wide wall-clock usage,
/// not just their own calls.
#[derive(Clone, Debug)]
pub struct CapabilityBudget {
    timeout_ms: u32,
    /// Wall-clock usage stats (best-effort, sampled). Surfaced
    /// by the HTTP bridge; not enforced beyond timeout.
    wall_clock_total_ms: Arc<AtomicU64>,
    /// Per-capability rate-limit accounting. Shared between the
    /// parent and all derived caps via `Arc`.
    quota_state: Arc<QuotaState>,
}

impl CapabilityBudget {
    /// New budget with the spec defaults (no quota) and a
    /// `SystemClock`.
    pub fn new(timeout_ms: u32) -> Self {
        Self::with_clock(
            timeout_ms,
            QuotaSpec::unlimited(),
            Arc::new(SystemClock),
        )
    }

    /// New budget with a quota spec and the system clock.
    pub fn with_spec(timeout_ms: u32, spec: QuotaSpec) -> Self {
        Self::with_clock(
            timeout_ms,
            spec,
            Arc::new(SystemClock),
        )
    }

    /// New budget with full control over the clock (production
    /// + tests).
    pub fn with_clock(timeout_ms: u32, spec: QuotaSpec, clock: Arc<dyn Clock>) -> Self {
        Self {
            timeout_ms,
            wall_clock_total_ms: Arc::new(AtomicU64::new(0)),
            quota_state: Arc::new(QuotaState::new(spec, clock)),
        }
    }

    /// Per-call wall-clock ceiling.
    pub fn timeout_ms(&self) -> u32 {
        self.timeout_ms
    }

    /// Load the current wall-clock total. Phase 5 D3
    /// verification: derived caps share the parent's
    /// `Arc<AtomicU64>` after the fix, so this returns the
    /// subtree total rather than a fresh counter.
    pub fn wall_clock_total_ms(&self) -> u64 {
        self.wall_clock_total_ms.load(Ordering::SeqCst)
    }

    /// Borrow the underlying quota state (snapshot only).
    /// Phase 5 n3 fix: `quota_state` is no longer `pub` —
    /// callers see only `snapshot()` and `try_call()` via
    /// `Capability`.
    pub fn snapshot(&self) -> QuotaSnapshot {
        self.quota_state.snapshot()
    }

    /// Spec the budget was constructed with. Used by the host
    /// at mint time to populate `CapabilityMeta::quota`.
    pub fn quota_spec(&self) -> QuotaSpec {
        self.quota_state.spec()
    }

    /// Try to debit a call. Returns `Err(QuotaKind)` if over
    /// quota. Internal: called from `Capability::invoke_op`
    /// and `Capability::open`.
    pub(crate) fn try_call(&self) -> Result<(), QuotaKind> {
        self.quota_state.try_call()
    }

    /// Atomically record elapsed wall-clock for one call. Phase
    /// 5 M3 fix: quota is debited AFTER the handler returns
    /// (not before), so a timeout or quota-exhaustion does not
    /// silently drop successful handler output.
    pub(crate) fn record_elapsed(&self, elapsed: std::time::Duration) {
        // Round up so that sub-millisecond calls register as ≥1ms.
        let elapsed_ms = std::cmp::max(1u128, elapsed.as_micros().div_ceil(1000)) as u64;
        self.wall_clock_total_ms.fetch_add(elapsed_ms, Ordering::Relaxed);
    }

    /// Construct a budget whose `QuotaState` AND wall-clock
    /// counter are **shared** with an existing budget. Phase 5
    /// D3 fix.
    pub fn share_with(parent: &CapabilityBudget, timeout_ms: u32) -> Self {
        Self {
            timeout_ms,
            wall_clock_total_ms: Arc::clone(&parent.wall_clock_total_ms),
            quota_state: Arc::clone(&parent.quota_state),
        }
    }
}
