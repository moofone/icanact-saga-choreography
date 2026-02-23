//! Choreography-Based SAGA for icanact-core Actors
//!
//! A choreography-based SAGA pattern that integrates natively with `icanact-core` actors.
//! Actors implement two traits (`SagaParticipant` + `SagaStateExt`) to participate in sagas.
//!
//! # Quick Start
//!
//! ```rust,ignore
//! // 1. Add saga state fields to your actor
//! pub struct MyActor {
//!     saga_states: HashMap<SagaId, SagaStateEntry>,
//!     saga_journal: Arc<dyn ParticipantJournal>,
//!     saga_dedupe: Arc<dyn ParticipantDedupeStore>,
//!     saga_stats: Arc<ParticipantStats>,
//! }
//!
//! // 2. Implement SagaStateExt (boilerplate)
//! impl SagaStateExt for MyActor { /* ... */ }
//!
//! // 3. Implement SagaParticipant (business logic)
//! impl SagaParticipant for MyActor { /* ... */ }
//!
//! // 4. Handle saga events in Actor::handle
//! MyActorCommand::SagaEvent { event } => handle_saga_event(self, event),
//! ```

#![warn(missing_docs)]

// === Core Types ===
mod context;
mod errors;
mod events;
mod idempotency;
mod state;

// === Traits ===
mod state_ext;
mod traits;

// === Storage ===
mod dedupe;
mod journal;

// === Observability ===
mod observer;
mod stats;

// === Helpers ===
mod helpers;

// === Re-exports ===

// Types
pub use context::{PeerId, SagaContext, SagaId, StepId};
pub use idempotency::IdempotencyKey;

// State (typestate)
pub use state::{
    Compensated, Compensating, Completed, Executing, Failed, Idle, Quarantined,
    SagaParticipantState, SagaStateEntry, TimestampedEvent, Triggered,
};

// Events
pub use events::{AckStatus, ParticipantEvent, SagaChoreographyEvent};

// Errors
pub use errors::{CompensationError, StepError, StepOutput};

// Traits
pub use state_ext::SagaStateExt;
pub use traits::{DependencySpec, RetryPolicy, SagaParticipant};

// Storage
pub use dedupe::{DedupeError, InMemoryDedupe, ParticipantDedupeStore};
pub use journal::{InMemoryJournal, JournalEntry, JournalError, ParticipantJournal};

// Observability
pub use observer::{NoOpObserver, SagaObserver, TracingObserver};
pub use stats::{ParticipantStats, ParticipantStatsSnapshot};

// Helpers
pub use helpers::{compensate_wrapper, execute_step_wrapper, handle_saga_event, recover_sagas};
