//! Capability kind — sync vs stream discriminator.

/// Runtime distinction between sync and streaming capabilities.
///
/// `R: Resource` does not carry this at compile time; the kind is
/// stamped at mint time and consulted at dispatch. A sync cap called
/// with `open` (or vice versa) returns a `CapabilityError::KindMismatch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapKind {
    Sync,
    Stream,
}
