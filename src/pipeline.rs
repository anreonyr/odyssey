//! Linear pipeline composition using capability tokens.
//!
//! Pipelines hold pre-resolved `Arc<CapabilityToken>` references — no string
//! lookup at runtime. Each stage invokes its token in sequence.

use std::sync::Arc;

use serde_json::Value;

use crate::capability::CapabilityToken;

#[derive(Debug, Clone)]
pub struct Pipeline(Vec<SyncStage>);

#[derive(Clone)]
pub struct SyncStage {
    token: Arc<CapabilityToken>,
}

impl std::fmt::Debug for SyncStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SyncStage::Invoke").field(&self.token.meta().name).finish()
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
    pub fn invoke(token: Arc<CapabilityToken>) -> Self {
        Self { token }
    }

    fn run(&self, input: Value) -> Result<Value, PipelineError> {
        self.token
            .invoke(input)
            .map_err(|e| PipelineError::StageFailed(self.token.name().to_string(), e))
    }
}

#[derive(Debug)]
pub enum PipelineError {
    StageFailed(String, String),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StageFailed(name, err) => {
                write!(f, "pipeline: stage \"{name}\" failed: {err}")
            }
        }
    }
}

impl std::error::Error for PipelineError {}