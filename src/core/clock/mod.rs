//! Clock — the wall-clock seam.
//!
//! Pure data: returns an `Instant` derived from whatever clock
//! the implementor chooses. Production uses `SystemClock`;
//! tests inject `MockClock` to make time deterministic.

pub mod clock;
