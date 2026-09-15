//! Capability metadata — `CapabilityMeta` + `CapabilityAction`.
//!
//! Phase 5: split from `capability::types`.
//!
//! Phase 9 cleanup: the `AuthorityContract` and `Protocol`
//! structs that used to live here are gone. Both were write-
//! only metadata — constructed by the manifest builder,
//! propagated into `CapabilityMeta` at mint, never read by
//! any code path. The audit flagged `operation_for` (the
//! `AuthorityContract`'s only reader) as dead; tracing the
//! reachability chain showed everything else in the
//! `authority`/`protocol` lineage was dead too. The remaining
//! shape is just `CapabilityMeta` (carries the cap's
//! identity + authority bits + namespace) and the `R::`
//! authority bitflags type that already lives in
//! `core::rights::rights`.

use crate::core::identity::ids::CapabilityId;
use crate::core::identity::ids::PluginId;
use crate::core::identity::kind::CapKind;
use crate::core::quota::quota::QuotaSpec;

/// Static description of a capability. Carried inside every token;
/// surfaced to the HTTP bridge for enumeration.
///
/// Phase 2 extends the surface with `namespace`, `contract`, and
/// `quota` so the HTTP bridge can render a real schema + rate limit
/// and `cspace.enumerate_namespace(prefix)` can return a scoped view.
#[derive(Clone, Debug)]
pub struct CapabilityMeta {
    pub id: CapabilityId,
    pub name: String,
    /// Hierarchical namespace this capability is filed under, e.g.
    /// `"odyssey.model.llama3"` or `"org.example.db.read"`. Empty
    /// string means "root namespace" (legacy single-name caps).
    pub namespace: String,
    /// Phase 3 P3.1 — the contract name this capability
    /// publishes. Set from the manifest's `[[exposes]] contract_name`
    /// and copied verbatim into the runtime meta so the resolver,
    /// HTTP bridge, and any introspection layer can match
    /// `requires[*].contract` against it. Empty string means
    /// "no contract published" (legacy caps; not reachable via
    /// capability injection).
    pub contract_name: String,
    pub plugin: PluginId,
    pub in_type: String,
    pub out_type: String,
    pub kind: CapKind,
    pub timeout_ms: u32,
    pub quota: QuotaSpec,
}
