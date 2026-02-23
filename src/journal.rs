//! Participant journal storage trait

use super::{SagaId, ParticipantEvent};
use serde::{Deserialize, Serialize};

/// Journal storage trait
pub trait ParticipantJournal: Send + Sync + 'static {
    fn append(&self, saga_id: SagaId, event: ParticipantEvent) -> Result<u64, JournalError>;
    fn read(&self, saga_id: SagaId) -> Result<Vec<JournalEntry>, JournalError>;
    fn list_sagas(&self) -> Result<Vec<SagaId>, JournalError>;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JournalEntry {
    pub sequence: u64,
    pub recorded_at_millis: u64,
    pub event: ParticipantEvent,
}

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("Storage error: {0}")]
    Storage(Box<str>),
    #[error("Not found: {0}")]
    NotFound(SagaId),
}

/// In-memory journal for testing
pub struct InMemoryJournal {
    data: std::sync::RwLock<std::collections::HashMap<u64, Vec<JournalEntry>>>,
    counter: std::sync::atomic::AtomicU64,
}

impl InMemoryJournal {
    pub fn new() -> Self {
        Self {
            data: std::sync::RwLock::new(std::collections::HashMap::new()),
            counter: std::sync::atomic::AtomicU64::new(1),
        }
    }
}

impl ParticipantJournal for InMemoryJournal {
    fn append(&self, saga_id: SagaId, event: ParticipantEvent) -> Result<u64, JournalError> {
        let seq = self.counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let entry = JournalEntry {
            sequence: seq,
            recorded_at_millis: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            event,
        };
        
        let mut data = self.data.write().map_err(|e| JournalError::Storage(e.to_string().into()))?;
        data.entry(saga_id.0).or_default().push(entry);
        
        Ok(seq)
    }
    
    fn read(&self, saga_id: SagaId) -> Result<Vec<JournalEntry>, JournalError> {
        let data = self.data.read().map_err(|e| JournalError::Storage(e.to_string().into()))?;
        Ok(data.get(&saga_id.0).cloned().unwrap_or_default())
    }
    
    fn list_sagas(&self) -> Result<Vec<SagaId>, JournalError> {
        let data = self.data.read().map_err(|e| JournalError::Storage(e.to_string().into()))?;
        Ok(data.keys().map(|&id| SagaId::new(id)).collect())
    }
}

impl Default for InMemoryJournal {
    fn default() -> Self {
        Self::new()
    }
}
