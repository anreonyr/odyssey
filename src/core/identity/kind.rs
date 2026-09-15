//! Capability kind — sync vs stream discriminator.

use serde::{Deserialize, Serialize};

/// Runtime distinction between sync and streaming capabilities.
///
/// `R: Resource` does not carry this at compile time; the kind is
/// stamped at mint time and consulted at dispatch. A sync cap called
/// with `open` (or vice versa) returns a `CapabilityError::KindMismatch`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapKind {
    #[default]
    Sync,
    Stream,
}
