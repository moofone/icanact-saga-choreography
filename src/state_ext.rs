//! Extension trait for saga state management.
//!
//! This module provides the [`SagaStateExt`] trait, which supplies common
//! boilerplate methods for managing saga state, deduplication, and journaling.
//! Implement this trait for types that need to participate in saga orchestration.

use crate::{ParticipantDedupeStore, ParticipantEvent, ParticipantJournal, SagaId, SagaStateEntry};
use std::collections::HashMap;

/// Extension trait providing common saga state management operations.
///
/// This trait defines the core interface for types that manage saga lifecycle
/// state, including access to state storage, journaling, and deduplication.
/// Implementors provide access to the underlying storage mechanisms, while
/// the trait supplies default implementations for common operations.
///
/// # Implementation Requirements
///
/// Implementors must provide:
/// - Mutable and immutable access to the saga state map
/// - Access to the participant journal for event persistence
/// - Access to the deduplication store for idempotency tracking
/// - A monotonic timestamp source for time-based operations
///
/// # Example
///
/// ```ignore
/// struct MyStateManager {
///     states: HashMap<SagaId, SagaStateEntry>,
///     journal: InMemoryJournal,
///     dedupe: InMemoryDedupe,
/// }
///
/// impl SagaStateExt for MyStateManager {
///     type Journal = InMemoryJournal;
///     type Dedupe = InMemoryDedupe;
///
///     fn saga_states(&mut self) -> &mut HashMap<SagaId, SagaStateEntry> {
///         &mut self.states
///     }
///     // ... implement other required methods
/// }
/// ```
pub trait SagaStateExt: Send + 'static {
    type Journal: ParticipantJournal;
    type Dedupe: ParticipantDedupeStore;

    /// Returns mutable access to the saga state map.
    ///
    /// This provides direct access to the underlying storage for saga state entries,
    /// allowing modifications such as inserting new sagas or updating existing ones.
    fn saga_states(&mut self) -> &mut HashMap<SagaId, SagaStateEntry>;

    /// Returns immutable access to the saga state map.
    ///
    /// Use this for read-only operations that need to inspect saga state
    /// without modifying it.
    fn saga_states_ref(&self) -> &HashMap<SagaId, SagaStateEntry>;

    /// Returns the participant journal for event persistence.
    ///
    /// The journal is used to durably record saga events for recovery
    /// and audit purposes.
    fn saga_journal(&self) -> &Self::Journal;

    /// Returns the deduplication store for idempotency tracking.
    ///
    /// The dedupe store tracks which operations have already been processed
    /// to prevent duplicate execution of side effects.
    fn saga_dedupe(&self) -> &Self::Dedupe;

    /// Returns the current timestamp in milliseconds.
    ///
    /// This should return a monotonically increasing value suitable for
    /// time-based operations such as timeouts and expiration checks.
    fn now_millis(&self) -> u64;

    /// Checks and marks a deduplication key for the given saga.
    ///
    /// Returns `true` if this is the first time the key has been seen for
    /// this saga (indicating the operation should proceed), or `false` if
    /// the key has already been processed (indicating the operation should
    /// be skipped as a duplicate).
    ///
    /// # Arguments
    ///
    /// * `saga_id` - The unique identifier of the saga
    /// * `key` - The deduplication key to check
    ///
    /// # Returns
    ///
    /// `true` if the operation is new and should proceed, `false` if it
    /// has already been processed.
    fn check_dedupe(&self, saga_id: SagaId, key: &str) -> bool {
        self.saga_dedupe()
            .check_and_mark(saga_id, key)
            .unwrap_or(false)
    }

    /// Records an event to the saga journal.
    ///
    /// Appends the given event to the durable journal for the specified saga.
    /// Errors during journaling are silently ignored; use this for best-effort
    /// event recording where durability is desired but not strictly required.
    ///
    /// # Arguments
    ///
    /// * `saga_id` - The unique identifier of the saga
    /// * `event` - The participant event to record
    fn record_event(&self, saga_id: SagaId, event: ParticipantEvent) {
        let _ = self.saga_journal().append(saga_id, event);
    }

    /// Removes all state associated with a saga.
    ///
    /// This removes the saga from the state map and prunes its deduplication
    /// entries. Use this when a saga has completed and its state is no longer
    /// needed.
    ///
    /// # Arguments
    ///
    /// * `saga_id` - The unique identifier of the saga to prune
    fn prune_saga(&mut self, saga_id: SagaId) {
        self.saga_states().remove(&saga_id);
        let _ = self.saga_dedupe().prune(saga_id);
    }

    /// Checks whether a saga is still actively running.
    ///
    /// Returns `true` if the saga exists and has not reached a terminal state,
    /// `false` otherwise (including if the saga does not exist).
    ///
    /// # Arguments
    ///
    /// * `saga_id` - The unique identifier of the saga to check
    ///
    /// # Returns
    ///
    /// `true` if the saga is active, `false` if completed, failed, or not found.
    fn is_saga_active(&self, saga_id: SagaId) -> bool {
        self.saga_states_ref()
            .get(&saga_id)
            .map(|e| !e.is_terminal())
            .unwrap_or(false)
    }

    /// Returns a list of all active saga identifiers.
    ///
    /// Collects and returns the IDs of all sagas that have not yet reached
    /// a terminal state. Useful for monitoring and cleanup operations.
    ///
    /// # Returns
    ///
    /// A vector containing the IDs of all active sagas.
    fn active_saga_ids(&self) -> Vec<SagaId> {
        self.saga_states_ref()
            .iter()
            .filter(|(_, entry)| !entry.is_terminal())
            .map(|(id, _)| *id)
            .collect()
    }

    /// Returns the count of currently active sagas.
    ///
    /// This is a convenience method that counts sagas that have not yet
    /// reached a terminal state.
    ///
    /// # Returns
    ///
    /// The number of active (non-terminal) sagas.
    fn active_saga_count(&self) -> usize {
        self.saga_states_ref()
            .values()
            .filter(|e| !e.is_terminal())
            .count()
    }
}
