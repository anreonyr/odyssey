//! Clock abstraction — the kernel takes a `Clock`, never `Instant::now()`
//! directly.
//!
//! Phase 5: this trait is the seam that lets the kernel algebraic core
//! stay wall-clock-free. Production code uses `SystemClock`; the
//! loom-tests feature injects a `MockClock` to make time deterministic
//! under loom's permutation engine.
//!
//! The original Phase 4 code called `Instant::now()` inline in
//! `Capability::invoke_op` and `QuotaState::try_call`. That couples
//! the algebraic core to `std::time`, which:
//!   - prevents pure-kernel tests from running under loom (loom
//!     intercepts the standard clock via its own runtime),
//!   - leaks an adapter concern into a pure-functional type.
//!
//! All `Instant::now()` callsites moved here.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

/// Wall-clock provider. Pure data: returns an `Instant` derived from
/// whatever clock the implementor chooses.
///
/// Implementations must be `Send + Sync` so the trait object can live
/// inside `CapabilitySpace::new` (which is `Clone`).
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

/// Real wall-clock. Used in production. Wraps the stdlib.
#[derive(Clone)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Test clock. Holds a single `Instant` behind a `Mutex`; advance via
/// `advance()`. The loom-tests gate uses this to make the
/// `Instant::now()` calls in quota eviction deterministic.
#[derive(Clone)]
pub struct MockClock {
    now: Arc<Mutex<Instant>>,
}

impl MockClock {
    /// New mock clock pinned to the wall clock at construction time.
    pub fn new() -> Self {
        Self {
            now: Arc::new(Mutex::new(Instant::now())),
        }
    }

    /// New mock clock pinned to `start`.
    pub fn pinned(start: Instant) -> Self {
        Self {
            now: Arc::new(Mutex::new(start)),
        }
    }

    /// Advance the mock clock by `delta`.
    pub fn advance(&self, delta: std::time::Duration) {
        let mut g = self.now.lock().expect("MockClock poisoned");
        *g = g.checked_add(delta).expect("Instant overflow in MockClock");
    }
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MockClock {
    fn now(&self) -> Instant {
        *self.now.lock().expect("MockClock poisoned")
    }
}
