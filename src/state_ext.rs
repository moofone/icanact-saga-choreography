//! Extension trait for saga state management

use crate::{ParticipantDedupeStore, ParticipantEvent, ParticipantJournal, SagaId, SagaStateEntry};
use std::collections::HashMap;
use std::sync::Arc;

pub trait SagaStateExt: Send + 'static {
    fn saga_states(&mut self) -> &mut HashMap<SagaId, SagaStateEntry>;

    fn saga_states_ref(&self) -> &HashMap<SagaId, SagaStateEntry>;

    fn saga_journal(&self) -> &Arc<dyn ParticipantJournal>;

    fn saga_dedupe(&self) -> &Arc<dyn ParticipantDedupeStore>;

    fn now_millis(&self) -> u64;

    fn check_dedupe(&self, saga_id: SagaId, key: &str) -> bool {
        self.saga_dedupe()
            .check_and_mark(saga_id, key)
            .unwrap_or(false)
    }

    fn record_event(&self, saga_id: SagaId, event: ParticipantEvent) {
        let _ = self.saga_journal().append(saga_id, event);
    }

    fn prune_saga(&mut self, saga_id: SagaId) {
        self.saga_states().remove(&saga_id);
        let _ = self.saga_dedupe().prune(saga_id);
    }

    fn is_saga_active(&self, saga_id: SagaId) -> bool {
        self.saga_states_ref()
            .get(&saga_id)
            .map(|e| !e.is_terminal())
            .unwrap_or(false)
    }

    fn active_saga_ids(&self) -> Vec<SagaId> {
        self.saga_states_ref()
            .iter()
            .filter(|(_, entry)| !entry.is_terminal())
            .map(|(id, _)| *id)
            .collect()
    }

    fn active_saga_count(&self) -> usize {
        self.saga_states_ref()
            .values()
            .filter(|e| !e.is_terminal())
            .count()
    }
}
