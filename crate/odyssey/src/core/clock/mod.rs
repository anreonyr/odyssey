//! Clock — the wall-clock seam.
//!
//! Pure data: returns an `Instant` derived from whatever clock
//! the implementor chooses. Production uses `SystemClock`;
//! tests inject `MockClock` to make time deterministic.

// The project enforces a strict-hierarchy rule: every folder
// `foo/` contains a single `foo.rs` (the inner file shares the
// folder name). That layout triggers clippy's
// `module_inception` warning; the convention is deliberate so
// callers can write `crate::core::clock::clock::Clock` rather
// than `crate::core::clock::Clock`. Suppress the warning at
// the source.
#[allow(clippy::module_inception)]
pub mod clock;
