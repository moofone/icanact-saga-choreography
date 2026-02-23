//! Order Placer Actor - SAGA PARTICIPANT (Step: prepare_order)
//!
//! This is a SYNC LOCAL actor that:
//! 1. Subscribes to SagaStarted events
//! 2. Builds the Deribit order payload with proper formatting
//! 3. Adds metadata (client_id, idempotency key, etc.)
//! 4. Emits StepCompleted with the prepared order
//!
//! This actor implements SagaParticipant trait.

use icanact_core::local_sync::{Actor, MailboxAddr, ReplyTo};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use icanact_saga_choreography::{
    SagaId, SagaContext, SagaChoreographyEvent, ParticipantEvent,
    SagaParticipant, SagaStateExt, DependencySpec, RetryPolicy,
    ParticipantJournal, ParticipantDedupeStore, ParticipantStats,
    SagaStateEntry, SagaParticipantState, TimestampedEvent,
    StepOutput, StepError, CompensationError,
    Idle, Executing, Completed,
};

/// Order Placer commands
#[derive(Debug)]
pub enum OrderPlacerCommand {
    /// Saga event from pubsub
    SagaEvent { event: SagaChoreographyEvent },
    
    /// Recover pending sagas
    RecoverSagas { reply_to: ReplyTo<Vec<SagaId>> },
    
    /// Get stats
    GetStats { reply_to: ReplyTo<icanact_saga_choreography::ParticipantStatsSnapshot> },
}

/// Prepared order data
#[derive(Clone, Debug)]
pub struct PreparedOrder {
    pub instrument: Box<str>,
    pub side: Box<str>,
    pub quantity: f64,
    pub order_type: Box<str>,
    pub price: Option<f64>,
    pub client_id: Box<str>,
    pub idempotency_key: Box<str>,
    pub post_only: bool,
    pub reduce_only: bool,
}

/// Order Placer Actor
pub struct OrderPlacerActor {
    // === Saga State ===
    saga_states: std::collections::HashMap<SagaId, SagaStateEntry>,
    
    // === Saga Infrastructure ===
    saga_journal: Arc<dyn ParticipantJournal>,
    saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    saga_stats: Arc<ParticipantStats>,
    
    // === Configuration ===
    client_id_prefix: Box<str>,
    
    // === Time ===
    clock: fn() -> u64,
}

impl OrderPlacerActor {
    pub fn new(
        saga_journal: Arc<dyn ParticipantJournal>,
        saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    ) -> Self {
        Self {
            saga_states: std::collections::HashMap::new(),
            saga_journal,
            saga_dedupe,
            saga_stats: Arc::new(ParticipantStats::new()),
            client_id_prefix: "my_client".into(),
            clock: || std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
    
    fn now_millis(&self) -> u64 {
        (self.clock)()
    }
    
    /// Prepare the order from saga payload
    fn prepare_order(&self, saga_payload: &[u8], saga_id: SagaId) -> Result<PreparedOrder, Box<str>> {
        // Deserialize the saga payload
        let payload: crate::saga::DeribitOrderPayload = bincode::deserialize(saga_payload)
            .map_err(|e| format!("deserialize_error: {}", e).into())?;
        
        // Build client-unique ID
        let client_id = format!("{}-{}", self.client_id_prefix, saga_id.0).into_boxed_str();
        
        // Build idempotency key (for Deribit)
        let idempotency_key = format!("saga:{}:order", saga_id.0).into_boxed_str();
        
        // Determine order flags based on position
        let post_only = true;  // Make only
        let reduce_only = payload.side == "sell";  // Sell = reduce only
        
        Ok(PreparedOrder {
            instrument: payload.instrument,
            side: payload.side,
            quantity: payload.quantity,
            order_type: payload.order_type,
            price: payload.price,
            client_id,
            idempotency_key,
            post_only,
            reduce_only,
        })
    }
    
    /// Handle incoming saga event
    fn handle_saga_event(&mut self, event: SagaChoreographyEvent) {
        let context = event.context();
        let now = self.now_millis();
        
        // Only handle deribit_order saga type
        if context.saga_type != "deribit_order" {
            return;
        }
        
        // Idempotency check
        let dedupe_key = format!("{}:{}", context.trace_id, event.event_type());
        if !self.saga_dedupe.check_and_mark(context.saga_id, &dedupe_key) {
            self.saga_stats.duplicate_events.fetch_add(1, Ordering::Relaxed);
            return;
        }
        
        self.saga_stats.events_received.fetch_add(1, Ordering::Relaxed);
        
        match event {
            // Start when saga starts (first step after risk approval)
            SagaChoreographyEvent::SagaStarted { payload, .. } => {
                self.execute_prepare_order(context.clone(), payload, now);
            }
            
            // Cleanup on terminal states
            SagaChoreographyEvent::SagaCompleted { .. } |
            SagaChoreographyEvent::SagaFailed { .. } |
            SagaChoreographyEvent::SagaQuarantined { .. } => {
                self.saga_states.remove(&context.saga_id);
                let _ = self.saga_dedupe.prune(context.saga_id);
            }
            
            _ => {}
        }
    }
    
    fn execute_prepare_order(&mut self, context: SagaContext, payload: Vec<u8>, now: u64) {
        let saga_id = context.saga_id;
        
        // Build state
        let state = SagaParticipantState::new(
            saga_id,
            context.saga_type.clone(),
            "prepare_order".into(),
            context.correlation_id,
            context.trace_id,
            context.initiator_peer_id,
            context.saga_started_at_millis,
        )
        .trigger("saga_started", now)
        .start_execution(now);
        
        // Persist
        self.saga_journal.append(saga_id, ParticipantEvent::StepExecutionStarted {
            attempt: 1,
            started_at_millis: now,
        }).ok();
        
        self.saga_states.insert(saga_id, SagaStateEntry::Executing(state));
        self.saga_stats.steps_started.fetch_add(1, Ordering::Relaxed);
        
        // Execute
        match self.prepare_order(&payload, saga_id) {
            Ok(prepared) => {
                let output = bincode::serialize(&prepared).unwrap_or_default();
                let compensation_data = bincode::serialize(&prepared.client_id).unwrap_or_default();
                
                // Complete step
                if let Some(SagaStateEntry::Executing(s)) = self.saga_states.remove(&saga_id) {
                    let new_state = s.complete(output.clone(), compensation_data, now);
                    self.saga_states.insert(saga_id, SagaStateEntry::Completed(new_state));
                }
                
                self.saga_journal.append(saga_id, ParticipantEvent::StepExecutionCompleted {
                    output: output.clone(),
                    compensation_data: vec![],
                    completed_at_millis: now,
                }).ok();
                
                self.saga_stats.steps_completed.fetch_add(1, Ordering::Relaxed);
                
                // TODO: Emit StepCompleted via pubsub
                tracing::info!(
                    saga_id = %saga_id,
                    client_id = %prepared.client_id,
                    "Order prepared"
                );
            }
            Err(e) => {
                // Fail step
                if let Some(SagaStateEntry::Executing(s)) = self.saga_states.remove(&saga_id) {
                    let new_state = s.fail(e.clone(), false, now);
                    self.saga_states.insert(saga_id, SagaStateEntry::Failed(new_state));
                }
                
                self.saga_stats.steps_failed.fetch_add(1, Ordering::Relaxed);
                
                tracing::error!(
                    saga_id = %saga_id,
                    error = %e,
                    "Failed to prepare order"
                );
            }
        }
    }
}

impl Actor for OrderPlacerActor {
    type Msg = OrderPlacerCommand;
    
    fn handle(&mut self, msg: Self::Msg) {
        match msg {
            OrderPlacerCommand::SagaEvent { event } => {
                self.handle_saga_event(event);
            }
            
            OrderPlacerCommand::RecoverSagas { reply_to } => {
                // TODO: Implement recovery
                let _ = reply_tell(reply_to, Vec::new());
            }
            
            OrderPlacerCommand::GetStats { reply_to } => {
                let _ = reply_tell(reply_to, self.saga_stats.snapshot());
            }
        }
    }
}
