//! Linear pipeline composition over typed capabilities.
//!
//! Each stage wraps an `Arc<dyn AnyCapability>` looked up from the
//! service registry; stage construction rejects streaming capabilities.

use std::sync::Arc;

use serde_json::Value;

use crate::capability::AnyCapability;

#[derive(Debug, Clone)]
pub struct Pipeline(Vec<SyncStage>);

#[derive(Clone)]
pub struct SyncStage {
    cap: Arc<dyn AnyCapability>,
}

impl std::fmt::Debug for SyncStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SyncStage").field(&self.cap.meta().name).finish()
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
    /// Wrap an erased capability. Rejects streaming capabilities.
    pub fn new(cap: Arc<dyn AnyCapability>) -> Result<Self, PipelineError> {
        if cap.is_streaming() {
            return Err(PipelineError::StreamingNotAllowed(cap.meta().name.clone()));
        }
        Ok(Self { cap })
    }

    fn run(&self, input: Value) -> Result<Value, PipelineError> {
        self.cap
            .invoke_dyn(input)
            .map_err(|e| PipelineError::StageFailed(self.cap.meta().name.clone(), e))
    }
}

#[derive(Debug)]
pub enum PipelineError {
    StreamingNotAllowed(String),
    StageFailed(String, String),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StreamingNotAllowed(name) => write!(
                f,
                "pipeline: capability \"{name}\" is streaming; not allowed in sync pipeline"
            ),
            Self::StageFailed(name, err) => {
                write!(f, "pipeline: stage \"{name}\" failed: {err}")
            }
        }
    }
}

impl std::error::Error for PipelineError {}