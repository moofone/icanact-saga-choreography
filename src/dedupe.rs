//! Participant deduplication storage trait

use super::SagaId;

/// Deduplication storage trait
pub trait ParticipantDedupeStore: Send + Sync + 'static {
    fn check_and_mark(&self, saga_id: SagaId, key: &str) -> Result<bool, DedupeError>;
    fn contains(&self, saga_id: SagaId, key: &str) -> bool;
    fn mark_processed(&self, saga_id: SagaId, key: &str) -> Result<(), DedupeError>;
    fn prune(&self, saga_id: SagaId) -> Result<(), DedupeError>;
}

#[derive(Debug, thiserror::Error)]
pub enum DedupeError {
    #[error("Storage error: {0}")]
    Storage(Box<str>),
}

/// In-memory dedupe store for testing
pub struct InMemoryDedupe {
    data: std::sync::RwLock<std::collections::HashSet<(u64, Box<str>)>>,
}

impl InMemoryDedupe {
    pub fn new() -> Self {
        Self {
            data: std::sync::RwLock::new(std::collections::HashSet::new()),
        }
    }
}

impl ParticipantDedupeStore for InMemoryDedupe {
    fn check_and_mark(&self, saga_id: SagaId, key: &str) -> Result<bool, DedupeError> {
        let entry = (saga_id.0, key.into());
        let mut data = self.data.write().map_err(|e| DedupeError::Storage(e.to_string().into()))?;
        Ok(data.insert(entry))
    }
    
    fn contains(&self, saga_id: SagaId, key: &str) -> bool {
        let data = self.data.read().ok();
        data.map(|d| d.contains(&(saga_id.0, key.into()))).unwrap_or(false)
    }
    
    fn mark_processed(&self, saga_id: SagaId, key: &str) -> Result<(), DedupeError> {
        let mut data = self.data.write().map_err(|e| DedupeError::Storage(e.to_string().into()))?;
        data.insert((saga_id.0, key.into()));
        Ok(())
    }
    
    fn prune(&self, saga_id: SagaId) -> Result<(), DedupeError> {
        let mut data = self.data.write().map_err(|e| DedupeError::Storage(e.to_string().into()))?;
        data.retain(|(id, _)| *id != saga_id.0);
        Ok(())
    }
}

impl Default for InMemoryDedupe {
    fn default() -> Self {
        Self::new()
    }
}
