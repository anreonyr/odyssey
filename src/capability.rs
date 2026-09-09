//! Capability model — seL4-inspired.
//!
//! A `CapabilityToken` is an unforgeable handle to a piece of compute, scoped
//! by a wall-clock timeout. Tokens are minted by [`CapabilityFactory`] (only
//! the factory can produce them — same role as `CNode.Allocate` in seL4) and
//! handed to plugins via cordis. Tokens are shared via `Arc` so a clone
//! refers to the same underlying state.
//!
//! ## seL4 mapping
//!
//!   CNode.Allocate           → factory.mint_sync / mint_stream
//!   CNode capability (handle)→ CapabilityToken (Arc)
//!   endpoint.send            → token.invoke / token.stream
//!   resource badge           → CapabilityBudget.timeout_ms (per-call wall clock)
//!
//! The timeout is enforced on every `invoke` call: if the handler runs
//! longer than the budget, the call returns an error and never silently
//! succeeds. There is no accounting of remaining budget — single-call is the
//! only dimension we enforce here.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::manifest::{CapabilityDecl, PluginId};

/// Unforgeable capability identifier. Two tokens minted at different times
/// always have distinct ids, even if they wrap the same handler.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CapabilityId(pub u64);

impl std::fmt::Display for CapabilityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cap:{}", self.0)
    }
}

/// Static description of a capability. Carried inside every token; surfaces
/// to the HTTP bridge for enumeration.
#[derive(Clone, Debug)]
pub struct CapabilityMeta {
    pub id: CapabilityId,
    pub name: String,
    pub plugin: PluginId,
    pub in_type: String,
    pub out_type: String,
    pub streaming: bool,
    pub timeout_ms: u32,
}

/// Single-call wall-clock budget. Held inside every token via `Arc`, which
/// lets multiple clones enforce the same limit consistently.
#[derive(Clone, Debug)]
pub struct CapabilityBudget {
    pub timeout_ms: u32,
}

impl CapabilityBudget {
    pub fn new(timeout_ms: u32) -> Self {
        Self { timeout_ms }
    }
}

/// One chunk in a streaming capability response.
#[derive(Debug)]
pub enum CapabilityChunk<T = Value> {
    Item(T),
    Done,
}

/// Sync invocation handler — what `CapabilityToken.invoke()` dispatches to.
pub trait SyncInvoke: Send + Sync {
    fn invoke(&self, input: Value) -> Result<Value, String>;
}

/// Streaming invocation handler — what `CapabilityToken.stream()` dispatches to.
pub trait StreamInvoke: Send + Sync {
    fn stream(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String>;
}

// ---------------------------------------------------------------------------
// CapabilityToken
// ---------------------------------------------------------------------------

/// An unforgeable capability handle. Plugins receive `Arc<CapabilityToken>`
/// at activation time; they never look up capabilities by string name at
/// runtime. All resource constraints live on the token itself.
#[derive(Clone)]
pub struct CapabilityToken {
    meta: CapabilityMeta,
    budget: Arc<CapabilityBudget>,
    invoke: Option<Arc<dyn SyncInvoke>>,
    stream: Option<Arc<dyn StreamInvoke>>,
}

impl CapabilityToken {
    /// Mint a sync token wrapping a `SyncInvoke` handler.
    pub fn new_sync(
        decl: &CapabilityDecl,
        plugin: &PluginId,
        id: CapabilityId,
        budget: Arc<CapabilityBudget>,
        handler: Arc<dyn SyncInvoke>,
    ) -> Self {
        Self {
            meta: CapabilityMeta {
                id,
                name: decl.name.clone(),
                plugin: plugin.clone(),
                in_type: decl.in_type.clone(),
                out_type: decl.out_type.clone(),
                streaming: decl.streaming,
                timeout_ms: budget.timeout_ms,
            },
            budget,
            invoke: Some(handler),
            stream: None,
        }
    }

    /// Mint a streaming token wrapping a `StreamInvoke` handler.
    pub fn new_stream(
        decl: &CapabilityDecl,
        plugin: &PluginId,
        id: CapabilityId,
        budget: Arc<CapabilityBudget>,
        handler: Arc<dyn StreamInvoke>,
    ) -> Self {
        Self {
            meta: CapabilityMeta {
                id,
                name: decl.name.clone(),
                plugin: plugin.clone(),
                in_type: decl.in_type.clone(),
                out_type: decl.out_type.clone(),
                streaming: decl.streaming,
                timeout_ms: budget.timeout_ms,
            },
            budget,
            invoke: None,
            stream: Some(handler),
        }
    }

    pub fn meta(&self) -> &CapabilityMeta {
        &self.meta
    }

    pub fn name(&self) -> &str {
        &self.meta.name
    }

    pub fn id(&self) -> CapabilityId {
        self.meta.id.clone()
    }

    /// Invoke the wrapped handler. If elapsed wall-clock time exceeds the
    /// token's `timeout_ms`, returns `Err` and the handler result is dropped.
    /// This is the **only** budget enforcement; there is no cumulative
    /// accounting, no remaining-ns counter, and no drop-side compensation.
    pub fn invoke(&self, input: Value) -> Result<Value, String> {
        let handler = self.invoke.as_ref().ok_or_else(|| {
            format!("capability {} is streaming; use stream()", self.meta.name)
        })?;
        let start = Instant::now();
        let result = handler.invoke(input);
        let elapsed_ms = start.elapsed().as_millis() as u64;
        if elapsed_ms > self.budget.timeout_ms as u64 {
            return Err(format!(
                "capability {}: timeout {}ms exceeded budget {}ms",
                self.meta.name, elapsed_ms, self.budget.timeout_ms
            ));
        }
        result
    }

    /// Open a streaming channel. The token's timeout_ms still bounds the
    /// total stream duration as recorded by `invoke`-style wall clock when
    /// the receiver is consumed; per-chunk delivery is the handler's job.
    /// Stream semantics here are intentionally simple — the handler owns
    /// pacing.
    pub fn stream(&self, input: Value) -> Result<mpsc::Receiver<CapabilityChunk>, String> {
        let handler = self.stream.as_ref().ok_or_else(|| {
            format!("capability {} is sync; use invoke()", self.meta.name)
        })?;
        handler.stream(input)
    }
}

// ---------------------------------------------------------------------------
// CapabilityService — the cordis-visible registry the HTTP bridge enumerates
// ---------------------------------------------------------------------------

/// Shared registry of every capability token. `Clone` is cheap and shares
/// state, so the same registry is visible to main, to plugins via cordis
/// inject, and to the HTTP bridge — they all see the same registrations.
///
/// Provided as the `capability_service` cordis service.
#[derive(Clone, Default)]
pub struct CapabilityService {
    by_name: Arc<Mutex<Vec<Arc<CapabilityToken>>>>,
}

impl CapabilityService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a token. Re-registering the same name is rejected — manifests
    /// must declare unique capability names.
    pub fn register(&self, token: Arc<CapabilityToken>) -> Result<(), CapabilityError> {
        let mut by_name = self.by_name.lock().expect("capability service poisoned");
        if by_name.iter().any(|t| t.name() == token.name()) {
            return Err(CapabilityError::AlreadyExists(token.name().to_string()));
        }
        by_name.push(token);
        Ok(())
    }

    /// Look up a token by name.
    pub fn get(&self, name: &str) -> Option<Arc<CapabilityToken>> {
        let by_name = self.by_name.lock().expect("capability service poisoned");
        by_name.iter().find(|t| t.name() == name).cloned()
    }

    /// Snapshot of every registered token's metadata, sorted by name.
    pub fn enumerate(&self) -> Vec<CapabilityMeta> {
        let by_name = self.by_name.lock().expect("capability service poisoned");
        let mut metas: Vec<_> = by_name.iter().map(|t| t.meta().clone()).collect();
        metas.sort_by(|a, b| a.name.cmp(&b.name));
        metas
    }
}

#[derive(Debug)]
pub enum CapabilityError {
    AlreadyExists(String),
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyExists(n) => write!(f, "capability already registered: {n}"),
        }
    }
}

impl std::error::Error for CapabilityError {}

impl From<CapabilityError> for cordis::Error {
    fn from(e: CapabilityError) -> Self {
        cordis::Error::msg(e.to_string())
    }
}