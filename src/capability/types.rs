//! Basic capability types — kind discriminator, identifiers, metadata,
//! rights, budget, chunks, and the error enum. No behaviour; just data.

use std::fmt;
use std::num::NonZeroU64;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::kernel::manifest::PluginId;

// ---------------------------------------------------------------------------
// Kinds
// ---------------------------------------------------------------------------

/// Runtime distinction between sync and streaming capabilities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapKind {
    Sync,
    Stream,
}

// ---------------------------------------------------------------------------
// Identifier
// ---------------------------------------------------------------------------

/// Unforgeable capability identifier (the `cap:0`, `cap:1` namespace).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityId(pub u64);

impl fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cap:{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Slot identifier
// ---------------------------------------------------------------------------

/// Stable position in a `CapabilitySpace`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SlotId(NonZeroU64);

impl SlotId {
    /// Construct a SlotId from a raw u64. Panics if `raw` is 0 (SlotIds
    /// are non-zero by construction; the CSpace allocator starts at 1).
    pub fn new(raw: u64) -> Self {
        Self(NonZeroU64::new(raw).expect("SlotId::new called with 0"))
    }

    pub fn raw(&self) -> u64 {
        self.0.get()
    }
}

impl fmt::Display for SlotId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "slot:{}", self.0)
    }
}

// ---------------------------------------------------------------------------
// Metadata
// ---------------------------------------------------------------------------

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
    pub streaming: bool,
    pub timeout_ms: u32,
    pub quota: QuotaSpec,
    /// Authority vocabulary (action → OperationRights map).
    /// Phase 3 P3.2 — extracted from the old `contract` field.
    /// `RuleAgent` reads `authority.operation_for(action)` to
    /// translate verbs to bits. Load-bearing at runtime.
    pub authority: AuthorityContract,
    /// Wire-protocol metadata (schemas, description, transport).
    /// Phase 3 P3.2 — split out of the old `contract` field.
    /// Pure metadata; the runtime never validates against it.
    pub protocol: Protocol,
}

// ---------------------------------------------------------------------------
// Contract (Phase 2)
// ---------------------------------------------------------------------------

/// JSON-Schema-style contract for a capability's input and output.
/// Stored as `serde_json::Value` so any schema dialect can ride along.
///
/// The contract is *descriptive*, not enforced at runtime — `Resource`
/// stays the runtime shape. But the contract lets the HTTP bridge
/// render a typed form, lets discovery answer "what does this cap
/// accept?", and lets future versions of `Resource` become generic
/// over `R + Contract`.
/// One entry in a capability's published action vocabulary. A
/// cap that wants type-agnostic callers (e.g. `RuleAgent`) to
/// dispatch to it publishes the list of action verbs it accepts
/// along with the `OperationRights` bit a caller must hold to
/// perform each one. The `operation` field is a string ("READ",
/// "WRITE", "EXECUTE", "ADMIN") rather than the bitflag itself
/// so the contract stays JSON-Schema-friendly and the bitflag's
/// object-map serde shape doesn't leak into manifests.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CapabilityAction {
    pub name: String,
    pub operation: String,
}

// ---------------------------------------------------------------------------
// Phase 3 P3.2 — split contract into AuthorityContract (action → op map) and
// Protocol (wire metadata). The two concerns had been folded into one
// `CapabilityContract`, but they serve different audiences:
//
//   - `AuthorityContract` is consumed by type-agnostic dispatchers
//     (the `RuleAgent`) to translate action verbs to OperationRights bits.
//     It's load-bearing for runtime authority checks: a missing entry here
//     means "the cap refuses this action"; an unknown bit means "the cap
//     publishes a vocabulary the agent can't honour."
//
//   - `Protocol` is pure metadata: schemas, description, transport hint,
//     wire version. It doesn't drive any dispatch path. The HTTP bridge
//     (and future OpenAPI/JSON-RPC descriptors) read it to advertise what
//     the cap accepts; the runtime never validates against it.
//
// `Resource::invoke(Value)` still takes raw JSON. Protocol is observable
// but not enforced.
// ---------------------------------------------------------------------------

/// Authority vocabulary published by a capability: the set of
/// action verbs a caller may invoke, each tagged with the
/// `OperationRights` bit the caller must hold.
///
/// This is the **only** part of the original
/// `CapabilityContract` that the runtime path actually
/// consulted — the `RuleAgent` reads it via
/// `AuthorityContract::operation_for(action)`. Splitting it
/// out makes "what authority do I need to perform X?" a
/// first-class question answerable from a small focused
/// struct, separate from "what does the wire look like?".
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AuthorityContract {
    /// Per-capability RPC vocabulary. Empty means "no enumerable
    /// action surface" — callers must already know how to talk to
    /// the cap, or the type-agnostic dispatcher will refuse to
    /// route to it.
    #[serde(default)]
    pub actions: Vec<CapabilityAction>,
}

impl AuthorityContract {
    pub fn empty() -> Self {
        Self::default()
    }

    /// Append an action to the vocabulary.
    pub fn with_action(mut self, name: impl Into<String>, operation: impl Into<String>) -> Self {
        self.actions.push(CapabilityAction {
            name: name.into(),
            operation: operation.into(),
        });
        self
    }

    /// Look up the operation bit string published for `action`.
    /// Returns `None` if the action isn't in the cap's vocabulary
    /// — callers (e.g. the type-agnostic `RuleAgent`) treat that
    /// as a "no such method" error.
    pub fn operation_for(&self, action: &str) -> Option<&str> {
        self.actions
            .iter()
            .find(|a| a.name == action)
            .map(|a| a.operation.as_str())
    }
}

/// Wire-protocol metadata published by a capability. **Does
/// not drive dispatch.** `Resource::invoke(Value)` accepts raw
/// JSON and the handler parses internally; this struct
/// describes the surface so external clients (HTTP bridge,
/// OpenAPI generators, JSON-RPC descriptors) can advertise
/// what the cap accepts without modifying the runtime.
///
/// All fields are optional with sensible defaults so an empty
/// `Protocol` means "no advertised metadata":
///
/// - `description`: free-text one-liner; surfaces in docs.
/// - `input_schema` / `output_schema`: JSON Schema (or any
///   dialect that rides along as `serde_json::Value`).
/// - `media_type`: wire encoding. Default empty string means
///   "not advertised"; the HTTP bridge defaults to
///   `application/json` for JSON-typed caps.
/// - `version`: wire format version, independent of the
///   plugin's own version. Bumping protocol version means the
///   byte layout changed, not the capability's behaviour.
/// - `transport`: hint of where this cap is reachable
///   (`in-process`, `http`, `grpc`, ...). Empty means "not
///   advertised". The HTTP bridge uses this to decide
///   whether to register a route; in-process caps skip it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Protocol {
    /// One-line description. Surfaces in HTTP bridge docs and
    /// OpenAPI generators.
    #[serde(default)]
    pub description: String,
    /// JSON Schema (or similar) for the input object.
    #[serde(default)]
    pub input_schema: Value,
    /// JSON Schema (or similar) for the output value.
    #[serde(default)]
    pub output_schema: Value,
    /// Wire encoding. Empty means "not advertised".
    #[serde(default)]
    pub media_type: String,
    /// Wire format version. Independent of the plugin's own
    /// version. Bumping protocol.version means the byte layout
    /// changed, not the capability's behaviour.
    #[serde(default)]
    pub version: String,
    /// Hint of where this cap is reachable (`in-process`,
    /// `http`, `grpc`, ...). Empty means "not advertised".
    #[serde(default)]
    pub transport: String,
}

impl Protocol {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = desc.into();
        self
    }

    pub fn with_input(mut self, schema: Value) -> Self {
        self.input_schema = schema;
        self
    }

    pub fn with_output(mut self, schema: Value) -> Self {
        self.output_schema = schema;
        self
    }

    pub fn with_media_type(mut self, mt: impl Into<String>) -> Self {
        self.media_type = mt.into();
        self
    }

    pub fn with_version(mut self, v: impl Into<String>) -> Self {
        self.version = v.into();
        self
    }

    pub fn with_transport(mut self, t: impl Into<String>) -> Self {
        self.transport = t.into();
        self
    }
}

// ---------------------------------------------------------------------------
// Quota (Phase 2)
// ---------------------------------------------------------------------------

/// Declarative rate-limit specification. All fields are optional; a
/// capability with no quota has no rate limit.
///
/// `QuotaSpec` is the *static* declaration (immutable per capability
/// derivation). `QuotaState` is the *dynamic* accounting object
/// (`Arc<RwLock<...>>`) shared between the parent and every child.
///
/// ## Sharing semantics
///
/// Because `QuotaState` is held inside `CapabilityBudget` via `Arc`,
/// every derived capability (after `restrict`) shares the same
/// accounting bucket. A child's call *consumes* the parent's budget.
/// That's the correct model — the parent's quota is the total
/// authority the subtree can spend, not per-leaf.
///
/// For per-leaf accounting, mint a fresh `CapabilityFactory::mint`
/// (a fresh `Arc<QuotaState>` is created at mint time).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuotaSpec {
    /// Maximum *calls* (sync invocations or stream opens) per minute.
    /// 0 means unlimited.
    pub calls_per_minute: u32,
    /// Maximum *tokens* the resource may emit per minute. Tokens are
    /// counted via the chunk `tokens_used` field for streams, and via
    /// the returned value's `tokens_used` field for sync calls.
    /// 0 means unlimited.
    pub tokens_per_minute: u64,
    /// Maximum *bytes* the resource may emit per minute (response body
    /// size). 0 means unlimited.
    pub bytes_per_minute: u64,
}

impl QuotaSpec {
    pub fn unlimited() -> Self {
        Self::default()
    }

    pub fn with_calls_per_minute(mut self, n: u32) -> Self {
        self.calls_per_minute = n;
        self
    }

    pub fn with_tokens_per_minute(mut self, n: u64) -> Self {
        self.tokens_per_minute = n;
        self
    }

    pub fn with_bytes_per_minute(mut self, n: u64) -> Self {
        self.bytes_per_minute = n;
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
            tokens_per_minute: match (self.tokens_per_minute, other.tokens_per_minute) {
                (0, x) | (x, 0) => x,
                (a, b) => a.min(b),
            },
            bytes_per_minute: match (self.bytes_per_minute, other.bytes_per_minute) {
                (0, x) | (x, 0) => x,
                (a, b) => a.min(b),
            },
        }
    }

    pub fn is_unlimited(&self) -> bool {
        self.calls_per_minute == 0
            && self.tokens_per_minute == 0
            && self.bytes_per_minute == 0
    }
}

/// Sliding-window quota accounting. The window is the past minute
/// (`Instant::now() - Duration::from_secs(60)`); old timestamps are
/// evicted on every check.
///
/// Three counters, each with its own window:
/// - `calls`: every check bumps it
/// - `tokens`: the caller passes the number consumed (sync result or
///   chunk token count)
/// - `bytes`: same idea for byte consumption
///
/// Held inside `CapabilityBudget` via `Arc`, so multiple derived caps
/// share the same accounting. The state is private to the kernel —
/// plugins read the *declaration* (`QuotaSpec`) via meta, not the state.
#[derive(Debug)]
pub struct QuotaState {
    spec: QuotaSpec,
    inner: RwLock<QuotaStateInner>,
}

#[derive(Debug, Default)]
struct QuotaStateInner {
    call_stamps: Vec<Instant>,
    token_stamps: Vec<(Instant, u64)>,
    byte_stamps: Vec<(Instant, u64)>,
    /// Last quota exhaustion, for diagnostics. Cleared on next successful check.
    last_exhausted: Option<QuotaKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaKind {
    Calls,
    Tokens,
    Bytes,
}

impl fmt::Display for QuotaKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Calls  => write!(f, "calls"),
            Self::Tokens => write!(f, "tokens"),
            Self::Bytes  => write!(f, "bytes"),
        }
    }
}

impl QuotaState {
    /// Construct an empty quota state for a given spec.
    pub fn new(spec: QuotaSpec) -> Self {
        Self {
            spec,
            inner: RwLock::new(QuotaStateInner::default()),
        }
    }

    pub fn spec(&self) -> QuotaSpec {
        self.spec
    }

    /// Try to consume one call. Returns `Err(QuotaKind)` if the per-minute
    /// limit would be exceeded.
    pub fn try_call(&self) -> Result<(), QuotaKind> {
        let mut g = self.inner.write().expect("quota poisoned");
        Self::evict(&mut g.call_stamps, Instant::now(), |stamps| stamps.len() as u32);
        let limit = self.spec.calls_per_minute;
        if limit == 0 {
            g.call_stamps.push(Instant::now());
            g.last_exhausted = None;
            return Ok(());
        }
        if g.call_stamps.len() as u32 >= limit {
            g.last_exhausted = Some(QuotaKind::Calls);
            return Err(QuotaKind::Calls);
        }
        g.call_stamps.push(Instant::now());
        g.last_exhausted = None;
        Ok(())
    }

    /// Try to consume `n` tokens.
    pub fn try_tokens(&self, n: u64) -> Result<(), QuotaKind> {
        let mut g = self.inner.write().expect("quota poisoned");
        Self::evict_pairs(&mut g.token_stamps, Instant::now());
        let limit = self.spec.tokens_per_minute;
        if limit == 0 || n == 0 {
            if n > 0 {
                g.token_stamps.push((Instant::now(), n));
            }
            g.last_exhausted = None;
            return Ok(());
        }
        let used: u64 = g.token_stamps.iter().map(|(_, n)| *n).sum();
        if used + n > limit {
            g.last_exhausted = Some(QuotaKind::Tokens);
            return Err(QuotaKind::Tokens);
        }
        g.token_stamps.push((Instant::now(), n));
        g.last_exhausted = None;
        Ok(())
    }

    /// Try to consume `n` bytes (same logic as tokens).
    pub fn try_bytes(&self, n: u64) -> Result<(), QuotaKind> {
        let mut g = self.inner.write().expect("quota poisoned");
        Self::evict_pairs(&mut g.byte_stamps, Instant::now());
        let limit = self.spec.bytes_per_minute;
        if limit == 0 || n == 0 {
            if n > 0 {
                g.byte_stamps.push((Instant::now(), n));
            }
            g.last_exhausted = None;
            return Ok(());
        }
        let used: u64 = g.byte_stamps.iter().map(|(_, n)| *n).sum();
        if used + n > limit {
            g.last_exhausted = Some(QuotaKind::Bytes);
            return Err(QuotaKind::Bytes);
        }
        g.byte_stamps.push((Instant::now(), n));
        g.last_exhausted = None;
        Ok(())
    }

    /// Snapshot used-this-minute for diagnostics / HTTP bridge.
    pub fn snapshot(&self) -> QuotaSnapshot {
        let g = self.inner.read().expect("quota poisoned");
        QuotaSnapshot {
            calls_used: g.call_stamps.len() as u32,
            tokens_used: g.token_stamps.iter().map(|(_, n)| *n).sum(),
            bytes_used: g.byte_stamps.iter().map(|(_, n)| *n).sum(),
            last_exhausted: g.last_exhausted,
        }
    }

    fn evict(stamps: &mut Vec<Instant>, now: Instant, len: fn(&Vec<Instant>) -> u32) {
        let cutoff = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);
        stamps.retain(|t| *t >= cutoff);
        let _ = len;
    }

    fn evict_pairs(stamps: &mut Vec<(Instant, u64)>, now: Instant) {
        let cutoff = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);
        stamps.retain(|(t, _)| *t >= cutoff);
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct QuotaSnapshot {
    pub calls_used: u32,
    pub tokens_used: u64,
    pub bytes_used: u64,
    pub last_exhausted: Option<QuotaKind>,
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

/// Single-call wall-clock budget. Held inside every token via `Arc`,
/// which lets multiple clones enforce the same limit consistently.
///
/// Phase 2 also carries a `QuotaState` (also Arc-shared) so rate
/// limits are accounted at the kernel level. Multiple derived caps
/// share the same `Arc<QuotaState>`, so the quota is a *single bucket*
/// for the entire subtree.
#[derive(Clone, Debug)]
pub struct CapabilityBudget {
    pub timeout_ms: u32,
    /// Wall-clock usage stats (best-effort, sampled). Surfaced by the
    /// HTTP bridge; not enforced beyond timeout.
    pub wall_clock_total_ms: Arc<AtomicU64>,
    /// Per-capability rate-limit accounting. Shared between the parent
    /// and all derived caps via `Arc`.
    pub quota_state: Arc<QuotaState>,
}

impl CapabilityBudget {
    pub fn new(timeout_ms: u32) -> Self {
        Self::with_spec(timeout_ms, QuotaSpec::unlimited())
    }

    pub fn with_spec(timeout_ms: u32, spec: QuotaSpec) -> Self {
        Self {
            timeout_ms,
            wall_clock_total_ms: Arc::new(AtomicU64::new(0)),
            quota_state: Arc::new(QuotaState::new(spec)),
        }
    }

    /// Load the current wall-clock total. Phase 5 D3 verification:
    /// derived caps share the parent's `Arc<AtomicU64>` after the fix,
    /// so this returns the subtree total rather than a fresh counter.
    pub fn wall_clock_total_ms(&self) -> u64 {
        self.wall_clock_total_ms.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Construct a budget whose `QuotaState` is **shared** with an
    /// existing budget. Used when the host mints a derived cap that
    /// should draw from the parent's quota bucket.
    pub fn share_quota_with(parent: &CapabilityBudget, timeout_ms: u32) -> Self {
        Self {
            timeout_ms,
            wall_clock_total_ms: Arc::new(AtomicU64::new(0)),
            quota_state: parent.quota_state.clone(),
        }
    }
}

// ---------------------------------------------------------------------------
// Rights
// ---------------------------------------------------------------------------

bitflags! {
    /// Per-call operations a capability permits. The kernel (CSpace) is
    /// the only thing that *creates* these; `Capability::invoke` consults
    /// the held rights at every call to reject an operation that was
    /// dropped by `restrict`.
    ///
    /// This is Phase 1's first real "authority": a bit you can subtract,
    /// bit you cannot expand, bit the resource can introspect.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct OperationRights: u32 {
        /// Read / observe the resource's state.
        const READ      = 1 << 0;
        /// Mutate the resource's state.
        const WRITE     = 1 << 1;
        /// Invoke an effect (start a computation, run an actor, ...).
        const EXECUTE   = 1 << 2;
        /// Lifecycle authority — re-grant, restrict, revoke.
        const ADMIN     = 1 << 3;
        /// Convenience: every bit set. Used when minting the root cap.
        const ALL       = Self::READ.bits() | Self::WRITE.bits()
                        | Self::EXECUTE.bits() | Self::ADMIN.bits();
    }
}

impl Default for OperationRights {
    fn default() -> Self {
        Self::ALL
    }
}

/// Rights attached to a capability. Supplied when deriving a child via
/// `grant`, `transfer`, or `restrict`. Two dimensions:
///
/// - `operations` — what the holder may *do* (`READ`/`WRITE`/...).
///   Attenuation (`restrict`) enforces `child ⊆ parent`.
/// - `timeout_ms` — wall-clock budget per call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilityRights {
    pub operations: OperationRights,
    pub timeout_ms: u32,
}

impl Default for CapabilityRights {
    fn default() -> Self {
        Self {
            operations: OperationRights::ALL,
            timeout_ms: 5000,
        }
    }
}

impl CapabilityRights {
    /// Construct an "all authority, default timeout" rights bag — what
    /// the host uses when minting a root capability.
    pub fn root(timeout_ms: u32) -> Self {
        Self {
            operations: OperationRights::ALL,
            timeout_ms,
        }
    }

    #[allow(dead_code)]
    pub fn with_timeout(mut self, ms: u32) -> Self {
        self.timeout_ms = ms;
        self
    }

    #[allow(dead_code)]
    pub fn with_operations(mut self, ops: OperationRights) -> Self {
        self.operations = ops;
        self
    }

    /// `self ⊇ other` — every bit and every budget ceiling in `other`
    /// is also present in `self`. The CSpace rejects any child whose
    /// rights are *not* a subset of its parent's.
    pub fn contains(&self, other: &CapabilityRights) -> bool {
        self.operations.contains(other.operations) && self.timeout_ms >= other.timeout_ms
    }

    /// Return the largest rights that is ≤ `self` AND ≤ `other` —
    /// i.e. the intersection. Used when `restrict` silently clamps a
    /// caller that asks for more than it has.
    pub fn intersect(&self, other: &CapabilityRights) -> CapabilityRights {
        CapabilityRights {
            operations: self.operations & other.operations,
            timeout_ms: self.timeout_ms.min(other.timeout_ms),
        }
    }
}

// ---------------------------------------------------------------------------
// Stream chunks
// ---------------------------------------------------------------------------

/// One chunk in a streaming capability response.
///
/// Phase 2 keeps the enum shape unchanged — cost accounting happens
/// at the *capability* layer (the kernel credits the parent quota
/// bucket), not inside the chunk. Resources that want their output
/// counted report the per-chunk token / byte cost through the
/// `Capability::open_with_quota` API; for the simple `open` path the
/// cost is `1 token / item`.
#[derive(Debug)]
pub enum CapabilityChunk<T = Value> {
    Item(T),
    Done,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors returned by capability operations on the CSpace.
#[derive(Debug)]
pub enum CapabilityError {
    AlreadyExists(String),
    /// The slot was empty or revoked when an operation tried to read it.
    SlotEmpty(SlotId),
    /// `restrict` (or `grant`) asked for rights the parent does not have.
    /// This is the "you cannot amplify authority" invariant.
    AttenuationViolation {
        from: SlotId,
        requested: OperationRights,
        held: OperationRights,
    },
    /// The capability was invoked with an operation bit it does not hold.
    OperationDenied {
        name: String,
        requested: OperationRights,
        held: OperationRights,
    },
    /// Phase 2: the rate-limit quota exhausted.
    QuotaExceeded {
        name: String,
        kind: QuotaKind,
    },
}

impl fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyExists(n) => write!(f, "capability already installed: {n}"),
            Self::SlotEmpty(s) => write!(f, "slot {} empty or revoked", s.raw()),
            Self::AttenuationViolation { from, requested, held } => write!(
                f,
                "attenuation violation at slot {}: requested {:?} not subset of {:?}",
                from.raw(),
                requested,
                held
            ),
            Self::OperationDenied { name, requested, held } => write!(
                f,
                "operation denied for capability \"{name}\": requested {:?} not in {:?}",
                requested,
                held
            ),
            Self::QuotaExceeded { name, kind } => {
                write!(f, "quota exhausted for capability \"{name}\": {kind:?}")
            }
        }
    }
}

impl std::error::Error for CapabilityError {}

impl From<CapabilityError> for cordis::Error {
    fn from(e: CapabilityError) -> Self {
        cordis::Error::msg(e.to_string())
    }
}
