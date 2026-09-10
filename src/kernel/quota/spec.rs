//! Quota declarations.
//!
//! Phase 5 D5: `tokens_per_minute` / `bytes_per_minute` removed.
//! `try_tokens` / `try_bytes` were defined but never enforced at the
//! kernel boundary (see `QuotaKind::Tokens`/`Bytes` deleted in this
//! same pass). Operators who set those limits in their manifests
//! get a `tracing::warn!` from the loader and the limits are dropped
//! on the floor; see `host::manifest::load`.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Declarative rate-limit specification. Phase 5 keeps only
/// `calls_per_minute` — the only field the kernel actually enforces.
///
/// `QuotaSpec` is the *static* declaration (immutable per capability
/// derivation). `QuotaState` is the *dynamic* accounting object
/// (`Arc<RwLock<...>>`) shared between the parent and every child.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaSpec {
    /// Maximum *calls* (sync invocations or stream opens) per minute.
    /// 0 means unlimited.
    pub calls_per_minute: u32,
}

impl QuotaSpec {
    pub fn unlimited() -> Self {
        Self::default()
    }

    pub fn with_calls_per_minute(mut self, n: u32) -> Self {
        self.calls_per_minute = n;
        self
    }

    /// A child quota is the *intersection* of parent and child
    /// (seL4 attenuation — you cannot amplify quota either).
    pub fn intersect(&self, other: &QuotaSpec) -> QuotaSpec {
        QuotaSpec {
            calls_per_minute: match (self.calls_per_minute, other.calls_per_minute) {
                (0, x) | (x, 0) => x, // 0 = unlimited; treat as identity
                (a, b) => a.min(b),
            },
        }
    }

    pub fn is_unlimited(&self) -> bool {
        self.calls_per_minute == 0
    }
}

/// Which quota axis a check exhausted. Phase 5 keeps only `Calls` —
/// `Tokens` / `Bytes` were dead in the runtime path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaKind {
    Calls,
}

impl fmt::Display for QuotaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Calls => write!(f, "calls"),
        }
    }
}

/// Snapshot used-this-minute for diagnostics / HTTP bridge.
#[derive(Clone, Copy, Debug, Default)]
pub struct QuotaSnapshot {
    pub calls_used: u32,
    /// Last quota exhaustion, for diagnostics. Cleared on next successful check.
    pub last_exhausted: Option<QuotaKind>,
}
