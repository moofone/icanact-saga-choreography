//! Order Coordinator Actor - SAGA PARTICIPANT (Step: place_order)
//!
//! This is a SYNC LOCAL actor that:
//! 1. Subscribes to StepCompleted from "prepare_order"
//! 2. Sends order to Deribit WS Actor via ASK
//! 3. Handles success/failure
//! 4. Triggers compensation if needed
//!
//! This is the CRITICAL saga step that interacts with external system.

use icanact_core::local_sync::{Actor, MailboxAddr, ReplyTo};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use icanact_saga_choreography::{
    SagaId, SagaContext, SagaChoreographyEvent, ParticipantEvent,
    SagaParticipant, SagaStateExt, DependencySpec, RetryPolicy, IdempotencyKey,
    ParticipantJournal, ParticipantDedupeStore, ParticipantStats,
    SagaStateEntry, SagaParticipantState,
    StepOutput, StepError, CompensationError,
    Executing, Completed, Compensating, Compensated, Quarantined,
};

use super::{DeribitWSCommand, DeribitWSResponse};

/// Order Coordinator commands
#[derive(Debug)]
pub enum OrderCoordinatorCommand {
    /// Saga event from pubsub
    SagaEvent { event: SagaChoreographyEvent },
    
    /// Deribit response (async callback from WS actor)
    OnDeribitResponse {
        saga_id: SagaId,
        response: Result<DeribitWSResponse, Box<str>>,
    },
    
    /// Recover pending sagas
    RecoverSagas { reply_to: ReplyTo<Vec<SagaId>> },
    
    /// Get stats
    GetStats { reply_to: ReplyTo<icanact_saga_choreography::ParticipantStatsSnapshot> },
    
    /// List active orders
    ListActiveOrders { reply_to: ReplyTo<Vec<(SagaId, Box<str>)>> },
}

/// Pending order state (waiting for Deribit response)
struct PendingOrder {
    context: SagaContext,
    prepared_order: PreparedOrderData,
    started_at: u64,
}

#[derive(Clone, Debug)]
struct PreparedOrderData {
    instrument: Box<str>,
    side: Box<str>,
    quantity: f64,
    client_id: Box<str>,
    idempotency_key: Box<str>,
    post_only: bool,
    reduce_only: bool,
}

/// Order Coordinator Actor
pub struct OrderCoordinatorActor {
    // === Dependencies ===
    deribit_ws: MailboxAddr<DeribitWSCommand>,
    saga_pubsub: icanact_core::local_sync::pubsub::PubSub<SagaChoreographyEvent>,
    
    // === Saga State ===
    saga_states: std::collections::HashMap<SagaId, SagaStateEntry>,
    
    // === Pending orders (waiting for async response) ===
    pending_orders: std::collections::HashMap<SagaId, PendingOrder>,
    
    // === Saga Infrastructure ===
    saga_journal: Arc<dyn ParticipantJournal>,
    saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    saga_stats: Arc<ParticipantStats>,
    
    // === Time ===
    clock: fn() -> u64,
}

impl OrderCoordinatorActor {
    pub fn new(
        deribit_ws: MailboxAddr<DeribitWSCommand>,
        saga_pubsub: icanact_core::local_sync::pubsub::PubSub<SagaChoreographyEvent>,
        saga_journal: Arc<dyn ParticipantJournal>,
        saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    ) -> Self {
        Self {
            deribit_ws,
            saga_pubsub,
            saga_states: std::collections::HashMap::new(),
            pending_orders: std::collections::HashMap::new(),
            saga_journal,
            saga_dedupe,
            saga_stats: Arc::new(ParticipantStats::new()),
            clock: || std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
    
    fn now_millis(&self) -> u64 {
        (self.clock)()
    }
    
    fn handle_saga_event(&mut self, event: SagaChoreographyEvent) {
        let context = event.context();
        let now = self.now_millis();
        
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
            // Triggered when "prepare_order" completes
            SagaChoreographyEvent::StepCompleted { step_name, output, .. }
                if step_name == "prepare_order"
            => {
                let next_context = context.next_step("place_order".into());
                self.execute_place_order(next_context, output, now);
            }
            
            // Compensation requested
            SagaChoreographyEvent::CompensationRequested { steps_to_compensate, .. } => {
                if steps_to_compensate.contains(&"place_order".into()) {
                    self.execute_compensation(context, now);
                }
            }
            
            // Cleanup
            SagaChoreographyEvent::SagaCompleted { .. } |
            SagaChoreographyEvent::SagaFailed { .. } |
            SagaChoreographyEvent::SagaQuarantined { .. } => {
                self.saga_states.remove(&context.saga_id);
                self.pending_orders.remove(&context.saga_id);
                let _ = self.saga_dedupe.prune(context.saga_id);
            }
            
            _ => {}
        }
    }
    
    fn execute_place_order(&mut self, context: SagaContext, prepared_data: Vec<u8>, now: u64) {
        let saga_id = context.saga_id;
        
        // Deserialize prepared order
        let prepared: PreparedOrderData = match bincode::deserialize(&prepared_data) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!(saga_id = %saga_id, error = %e, "Failed to deserialize prepared order");
                self.fail_step(&context, format!("deserialize_error: {}", e).into(), false, now);
                return;
            }
        };
        
        // Build state
        let state = SagaParticipantState::new(
            saga_id,
            context.saga_type.clone(),
            "place_order".into(),
            context.correlation_id,
            context.trace_id,
            context.initiator_peer_id,
            context.saga_started_at_millis,
        )
        .trigger("prepare_order_completed", now)
        .start_execution(now);
        
        // Persist
        self.saga_journal.append(saga_id, ParticipantEvent::StepExecutionStarted {
            attempt: 1,
            started_at_millis: now,
        }).ok();
        
        self.saga_states.insert(saga_id, SagaStateEntry::Executing(state));
        self.saga_stats.steps_started.fetch_add(1, Ordering::Relaxed);
        
        // Store pending order (for async response)
        self.pending_orders.insert(saga_id, PendingOrder {
            context: context.clone(),
            prepared_order: prepared.clone(),
            started_at: now,
        });
        
        // Send to Deribit WS Actor via ASK
        // Note: In a sync actor, we'd use ask_timeout which blocks
        // For async interaction, we'd use tell + callback pattern
        match self.deribit_ws.ask_timeout(
            |reply_to| DeribitWSCommand::PlaceOrder {
                instrument: prepared.instrument.clone(),
                side: prepared.side.clone(),
                quantity: prepared.quantity,
                order_type: "limit".into(),
                price: None,  // Market for now
                client_id: prepared.client_id.clone(),
                idempotency_key: prepared.idempotency_key.clone(),
                post_only: prepared.post_only,
                reduce_only: prepared.reduce_only,
                reply_to,
            },
            Duration::from_secs(10),
        ) {
            Ok(response) => {
                // Got response synchronously
                self.pending_orders.remove(&saga_id);
                self.handle_deribit_response(saga_id, Ok(response), now);
            }
            Err(e) => {
                // Ask failed (timeout, actor stopped)
                self.pending_orders.remove(&saga_id);
                self.handle_deribit_response(saga_id, Err(format!("ask_failed: {:?}", e).into()), now);
            }
        }
    }
    
    /// Handle response from Deribit (can be called async)
    pub fn handle_deribit_response(
        &mut self,
        saga_id: SagaId,
        response: Result<DeribitWSResponse, Box<str>>,
        now: u64,
    ) {
        // Get pending order
        let pending = match self.pending_orders.remove(&saga_id) {
            Some(p) => p,
            None => {
                // Already handled or unknown saga
                return;
            }
        };
        
        let context = pending.context;
        
        match response {
            Ok(ws_response) => {
                match ws_response {
                    DeribitWSResponse::OrderPlaced { order_id } => {
                        // SUCCESS
                        let output = bincode::serialize(&order_id).unwrap_or_default();
                        let compensation_data = bincode::serialize(&order_id).unwrap_or_default();
                        
                        self.complete_step(&context, output, compensation_data, now);
                        
                        // Emit StepCompleted
                        self.emit_step_completed(&context, order_id);
                        
                        tracing::info!(
                            saga_id = %saga_id,
                            order_id = %order_id,
                            "Order placed successfully"
                        );
                    }
                    DeribitWSResponse::OrderRejected { reason } => {
                        // FAILED - check if retriable
                        let is_retriable = reason.contains("rate_limit") || reason.contains("timeout");
                        
                        if is_retriable {
                            self.fail_step(&context, reason.clone(), false, now);
                        } else {
                            // Permanent failure - may need compensation for previous steps
                            self.fail_step(&context, reason.clone(), true, now);
                            self.emit_compensation_requested(&context, reason);
                        }
                    }
                    DeribitWSResponse::Error { message } => {
                        self.fail_step(&context, message.clone(), true, now);
                        self.emit_compensation_requested(&context, message);
                    }
                }
            }
            Err(e) => {
                // Communication error
                self.fail_step(&context, e.clone(), true, now);
                self.emit_compensation_requested(&context, e);
            }
        }
    }
    
    fn execute_compensation(&mut self, context: &SagaContext, now: u64) {
        let saga_id = context.saga_id;
        
        // Get compensation data (order_id) from Completed state
        if let Some(SagaStateEntry::Completed(state)) = self.saga_states.remove(&saga_id) {
            let order_id: Box<str> = bincode::deserialize(&state.state.compensation_data)
                .unwrap_or_else(|_| "unknown".into());
            
            // Transition to Compensating
            let new_state = state.start_compensation(now);
            self.saga_states.insert(saga_id, SagaStateEntry::Compensating(new_state));
            
            // Persist
            self.saga_journal.append(saga_id, ParticipantEvent::CompensationStarted {
                attempt: 1,
                started_at_millis: now,
            }).ok();
            
            self.saga_stats.compensations_started.fetch_add(1, Ordering::Relaxed);
            
            // Cancel order via WS actor
            match self.deribit_ws.ask_timeout(
                |reply_to| DeribitWSCommand::CancelOrder {
                    order_id: order_id.clone(),
                    reply_to,
                },
                Duration::from_secs(5),
            ) {
                Ok(DeribitWSResponse::OrderCancelled) => {
                    self.complete_compensation(context, now);
                    tracing::info!(saga_id = %saga_id, order_id = %order_id, "Compensation completed");
                }
                Ok(DeribitWSResponse::OrderRejected { reason }) |
                Ok(DeribitWSResponse::Error { message: reason }) => {
                    // Ambiguous - order might or might not be cancelled
                    self.quarantine(context, format!("cancel_failed: {}", reason).into(), now);
                }
                Err(e) => {
                    self.quarantine(context, format!("cancel_ask_failed: {:?}", e).into(), now);
                }
                _ => {
                    self.quarantine(context, "unexpected_cancel_response".into(), now);
                }
            }
        }
    }
    
    fn complete_step(&mut self, context: &SagaContext, output: Vec<u8>, compensation_data: Vec<u8>, now: u64) {
        let saga_id = context.saga_id;
        
        if let Some(SagaStateEntry::Executing(state)) = self.saga_states.remove(&saga_id) {
            let new_state = state.complete(output, compensation_data, now);
            self.saga_states.insert(saga_id, SagaStateEntry::Completed(new_state));
        }
        
        self.saga_journal.append(saga_id, ParticipantEvent::StepExecutionCompleted {
            output: vec![],
            compensation_data: vec![],
            completed_at_millis: now,
        }).ok();
        
        self.saga_stats.steps_completed.fetch_add(1, Ordering::Relaxed);
    }
    
    fn fail_step(&mut self, context: &SagaContext, error: Box<str>, requires_comp: bool, now: u64) {
        let saga_id = context.saga_id;
        
        if let Some(SagaStateEntry::Executing(state)) = self.saga_states.remove(&saga_id) {
            let new_state = state.fail(error.clone(), requires_comp, now);
            self.saga_states.insert(saga_id, SagaStateEntry::Failed(new_state));
        }
        
        self.saga_journal.append(saga_id, ParticipantEvent::StepExecutionFailed {
            error,
            requires_compensation: requires_comp,
            failed_at_millis: now,
        }).ok();
        
        self.saga_stats.steps_failed.fetch_add(1, Ordering::Relaxed);
    }
    
    fn complete_compensation(&mut self, context: &SagaContext, now: u64) {
        let saga_id = context.saga_id;
        
        if let Some(SagaStateEntry::Compensating(state)) = self.saga_states.remove(&saga_id) {
            let new_state = state.complete_compensation(now);
            self.saga_states.insert(saga_id, SagaStateEntry::Compensated(new_state));
        }
        
        self.saga_journal.append(saga_id, ParticipantEvent::CompensationCompleted {
            completed_at_millis: now,
        }).ok();
        
        self.saga_stats.compensations_completed.fetch_add(1, Ordering::Relaxed);
    }
    
    fn quarantine(&mut self, context: &SagaContext, reason: Box<str>, now: u64) {
        let saga_id = context.saga_id;
        
        if let Some(SagaStateEntry::Compensating(state)) = self.saga_states.remove(&saga_id) {
            let new_state = state.quarantine(reason.clone(), now);
            self.saga_states.insert(saga_id, SagaStateEntry::Quarantined(new_state));
        }
        
        self.saga_journal.append(saga_id, ParticipantEvent::Quarantined {
            reason: reason.clone(),
            quarantined_at_millis: now,
        }).ok();
        
        self.saga_stats.quarantined_sagas.fetch_add(1, Ordering::Relaxed);
        
        tracing::error!(
            saga_id = %saga_id,
            reason = %reason,
            "Saga quarantined - manual intervention required"
        );
    }
    
    fn emit_step_completed(&self, context: &SagaContext, order_id: Box<str>) {
        let event = SagaChoreographyEvent::StepCompleted {
            context: context.clone(),
            output: bincode::serialize(&order_id).unwrap_or_default(),
            compensation_available: true,
        };
        self.saga_pubsub.publish("saga:deribit_order", event);
    }
    
    fn emit_compensation_requested(&self, context: &SagaContext, reason: Box<str>) {
        let event = SagaChoreographyEvent::CompensationRequested {
            context: context.clone(),
            failed_step: "place_order".into(),
            reason,
            steps_to_compensate: vec!["place_order".into()],
        };
        self.saga_pubsub.publish("saga:deribit_order", event);
    }
}

impl Actor for OrderCoordinatorActor {
    type Msg = OrderCoordinatorCommand;
    
    fn handle(&mut self, msg: Self::Msg) {
        match msg {
            OrderCoordinatorCommand::SagaEvent { event } => {
                self.handle_saga_event(event);
            }
            
            OrderCoordinatorCommand::OnDeribitResponse { saga_id, response } => {
                let now = self.now_millis();
                self.handle_deribit_response(saga_id, response, now);
            }
            
            OrderCoordinatorCommand::RecoverSagas { reply_to } => {
                let _ = reply_tell(reply_to, Vec::new());
            }
            
            OrderCoordinatorCommand::GetStats { reply_to } => {
                let _ = reply_tell(reply_to, self.saga_stats.snapshot());
            }
            
            OrderCoordinatorCommand::ListActiveOrders { reply_to } => {
                let orders: Vec<_> = self.pending_orders
                    .iter()
                    .map(|(id, pending)| (*id, pending.prepared_order.client_id.clone()))
                    .collect();
                let _ = reply_tell(reply_to, orders);
            }
        }
    }
}
