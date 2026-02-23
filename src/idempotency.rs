//! Idempotency key generation for saga steps

use crate::SagaId;
use serde::{Deserialize, Serialize};

/// Idempotency key for deduplicating side effects
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey(pub Box<str>);

impl IdempotencyKey {
    /// Create an idempotency key for a step execution
    pub fn for_step(saga_id: SagaId, step_name: &str, attempt: u32) -> Self {
        Self(format!("saga:{}:step:{}:attempt:{}", saga_id.0, step_name, attempt).into_boxed_str())
    }
    
    /// Create an idempotency key for compensation
    pub fn for_compensation(saga_id: SagaId, step_name: &str) -> Self {
        Self(format!("saga:{}:compensate:{}", saga_id.0, step_name).into_boxed_str())
    }
    
    /// Get the key as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for IdempotencyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
