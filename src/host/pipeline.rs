//! Linear pipeline composition over typed capabilities.
//!
//! Two ways to build a `SyncStage`:
//!
//! 1. `SyncStage::new(cap)` — wrap an `Arc<dyn AnyCapability>` you
//!    already hold. The stage pins the capability; revoking the slot
//!    it came from does **not** affect this stage.
//!
//! 2. `SyncStage::from_slot(slot, cspace)` — remember the slot id and
//!    the `CapabilitySpace`; re-resolve at every `run`. This is the
//!    revocation-observant form: `cspace.revoke(slot)` immediately
//!    makes the next pipeline run fail with `SlotRevoked`.
//!
//! Lab 04 (composition) uses the second form to demonstrate that
//! capability revocation propagates through composed stages without
//! changing the pipeline itself.

use std::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::kernel::error::CapabilityError;
use crate::kernel::{AnyCapability, CapabilitySpace, SlotId};

#[derive(Debug, Clone)]
pub struct Pipeline(Vec<SyncStage>);

#[derive(Clone)]
pub enum SyncStage {
    /// Static snapshot — capability pinned for the pipeline lifetime.
    Pinned { cap: Arc<dyn AnyCapability>, name: String },
    /// Slot-bound — re-resolves through the CSpace on every run.
    Slot {
        cspace: CapabilitySpace,
        slot: SlotId,
        name: String,
    },
}

impl fmt::Debug for SyncStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pinned { name, .. } => f.debug_tuple("SyncStage::Pinned").field(name).finish(),
            Self::Slot { name, .. }  => f.debug_tuple("SyncStage::Slot").field(name).finish(),
        }
    }
}

impl Pipeline {
    pub fn new(stages: Vec<SyncStage>) -> Self {
        Self(stages)
    }

    pub fn run(&self, input: Value) -> Result<Value, PipelineError> {
        let mut current = input;
        for stage in &self.0 {
            current = stage.run(current)?;
        }
        Ok(current)
    }
}

impl SyncStage {
    /// Static snapshot stage. The capability is pinned; revocation of
    /// the underlying slot does not affect this stage.
    pub fn new(cap: Arc<dyn AnyCapability>) -> Result<Self, PipelineError> {
        if cap.is_streaming() {
            return Err(PipelineError::StreamingNotAllowed(cap.meta().name.clone()));
        }
        let name = cap.meta().name.clone();
        Ok(Self::Pinned { cap, name })
    }

    /// Slot-bound stage. Re-resolves on every run, so revoking the
    /// slot makes the next run fail with `SlotRevoked` without
    /// touching the pipeline.
    pub fn from_slot(slot: SlotId, cspace: CapabilitySpace) -> Result<Self, PipelineError> {
        let meta = cspace
            .slot_meta(slot)
            .ok_or_else(|| PipelineError::SlotRevoked(slot.to_string()))?;
        if meta.streaming {
            return Err(PipelineError::StreamingNotAllowed(meta.name));
        }
        Ok(Self::Slot {
            cspace,
            slot,
            name: meta.name,
        })
    }

    fn run(&self, input: Value) -> Result<Value, PipelineError> {
        match self {
            Self::Pinned { cap, name } => cap.invoke_dyn_typed(input).map_err(|e| match e {
                CapabilityError::Revoked(s) | CapabilityError::SlotEmpty(s) => {
                    PipelineError::SlotRevoked(s.to_string())
                }
                other => PipelineError::StageFailed(name.clone(), other.to_string()),
            }),
            Self::Slot { cspace, slot, name } => {
                // Fresh lookup every call — this is what makes
                // revocation observable from inside the pipeline.
                let cap = cspace
                    .lookup_erased(*slot)
                    .ok_or_else(|| PipelineError::SlotRevoked(slot.to_string()))?;
                // Phase 5 M4: typed error inspection. The typed
                // `CapabilityError` variants let us pattern-match
                // on `Revoked(SlotId)` / `SlotEmpty(SlotId)` to
                // decide whether the run failed because the slot
                // was revoked mid-flight (PipelineError::SlotRevoked)
                // or for some other reason (PipelineError::StageFailed).
                // No substring matching on Display output.
                cap.invoke_dyn_typed(input).map_err(|e| match e {
                    CapabilityError::Revoked(s) | CapabilityError::SlotEmpty(s) => {
                        PipelineError::SlotRevoked(s.to_string())
                    }
                    other => PipelineError::StageFailed(name.clone(), other.to_string()),
                })
            }
        }
    }
}

#[derive(Debug)]
pub enum PipelineError {
    StreamingNotAllowed(String),
    StageFailed(String, String),
    /// The underlying slot was empty / revoked during `run`.
    SlotRevoked(String),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StreamingNotAllowed(name) => write!(
                f,
                "pipeline: capability \"{name}\" is streaming; not allowed in sync pipeline"
            ),
            Self::StageFailed(name, err) => {
                write!(f, "pipeline: stage \"{name}\" failed: {err}")
            }
            Self::SlotRevoked(slot) => {
                write!(f, "pipeline: slot {slot} revoked mid-run")
            }
        }
    }
}

impl std::error::Error for PipelineError {}