//! Risk Manager Actor - SAGA INITIATOR
//!
//! This is a SYNC LOCAL actor that:
//! 1. Receives TA signals
//! 2. Evaluates risk (position limits, exposure, rate limits)
//! 3. If GO: publishes SagaStarted event to start the order saga
//! 4. Tracks blocking to prevent multiple concurrent orders
//!
//! This actor PARTICIPATES in the saga as the coordinator/observer.

use icanact_core::local_sync::{Actor, MailboxAddr, ReplyTo, pubsub::PubSub};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::collections::HashSet;

use crate::saga::{DeribitOrderSagaEvent, DeribitOrderPayload, OrderSagaState};
use icanact_saga_choreography::{
    SagaId, SagaContext, SagaChoreographyEvent, ParticipantEvent,
    SagaParticipant, SagaStateExt, DependencySpec,
    ParticipantJournal, ParticipantDedupeStore, ParticipantStats,
    SagaStateEntry, SagaParticipantState,
    StepOutput, StepError, CompensationError,
};

/// Risk decision
#[derive(Clone, Debug)]
pub enum RiskDecision {
    /// Approved - proceed with order
    Go {
        instrument: Box<str>,
        side: OrderSide,
        quantity: f64,
        max_price: Option<f64>,
    },
    /// Rejected - do not proceed
    NoGo { reason: Box<str> },
}

#[derive(Clone, Debug)]
pub enum OrderSide {
    Buy,
    Sell,
}

/// Risk Manager commands
#[derive(Debug)]
pub enum RiskManagerCommand {
    /// Incoming signal from TA actor
    OnSignal { event: super::ta_signal::SignalEvent },
    
    /// Check if order is blocked (already in progress)
    IsBlocked {
        instrument: Box<str>,
        reply_to: ReplyTo<bool>,
    },
    
    /// Get current risk metrics
    GetMetrics { reply_to: ReplyTo<RiskMetrics> },
    
    /// Saga event from pubsub
    SagaEvent { event: SagaChoreographyEvent },
    
    /// Clear blocking for instrument
    ClearBlock { instrument: Box<str> },
}

/// Risk metrics snapshot
#[derive(Clone, Debug)]
pub struct RiskMetrics {
    pub orders_in_flight: usize,
    pub total_exposure: f64,
    pub orders_rejected: u64,
    pub orders_approved: u64,
}

/// Risk Manager Actor - SAGA INITIATOR
pub struct RiskManagerActor {
    // === Dependencies (other actors) ===
    rate_limiter: MailboxAddr<super::RateLimiterCommand>,
    
    // === PubSub for saga events ===
    saga_pubsub: PubSub<SagaChoreographyEvent>,
    
    // === Risk State ===
    /// Instruments with orders in flight (blocking)
    blocked_instruments: HashSet<Box<str>>,
    
    /// Current total exposure
    total_exposure: f64,
    
    /// Position limits
    max_exposure: f64,
    max_orders_per_minute: u32,
    
    // === Saga State (as initiator/observer) ===
    /// Active sagas we initiated
    active_sagas: std::collections::HashMap<SagaId, OrderSagaState>,
    
    /// Saga infrastructure
    saga_journal: Arc<dyn ParticipantJournal>,
    saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    saga_stats: Arc<ParticipantStats>,
    
    // === Metrics ===
    orders_approved: AtomicU64,
    orders_rejected: AtomicU64,
    
    // === Time ===
    clock: fn() -> u64,
}

impl RiskManagerActor {
    pub fn new(
        rate_limiter: MailboxAddr<super::RateLimiterCommand>,
        saga_pubsub: PubSub<SagaChoreographyEvent>,
        saga_journal: Arc<dyn ParticipantJournal>,
        saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    ) -> Self {
        Self {
            rate_limiter,
            saga_pubsub,
            blocked_instruments: HashSet::new(),
            total_exposure: 0.0,
            max_exposure: 100_000.0,
            max_orders_per_minute: 10,
            active_sagas: std::collections::HashMap::new(),
            saga_journal,
            saga_dedupe,
            saga_stats: Arc::new(ParticipantStats::new()),
            orders_approved: AtomicU64::new(0),
            orders_rejected: AtomicU64::new(0),
            clock: || std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
    
    fn now_millis(&self) -> u64 {
        (self.clock)()
    }
    
    /// Evaluate signal and decide GO/NOGO
    fn evaluate_signal(&mut self, event: &super::ta_signal::SignalEvent) -> RiskDecision {
        let instrument = match &event.signal_type {
            super::ta_signal::SignalType::Buy { instrument, .. } => instrument.clone(),
            super::ta_signal::SignalType::Sell { instrument, .. } => instrument.clone(),
            super::ta_signal::SignalType::Exit { .. } => {
                // Exit signals always GO
                return RiskDecision::Go {
                    instrument: "exit".into(),
                    side: OrderSide::Sell,
                    quantity: 0.0,
                    max_price: None,
                };
            }
        };
        
        // Check 1: Is this instrument blocked (order in flight)?
        if self.blocked_instruments.contains(&instrument) {
            self.orders_rejected.fetch_add(1, Ordering::Relaxed);
            return RiskDecision::NoGo {
                reason: "instrument_blocked".into(),
            };
        }
        
        // Check 2: Check rate limiter
        // (This would be an ask to the rate limiter actor in real impl)
        // For now, assume it's OK
        
        // Check 3: Exposure limits
        let order_value = 10_000.0; // Simplified
        if self.total_exposure + order_value > self.max_exposure {
            self.orders_rejected.fetch_add(1, Ordering::Relaxed);
            return RiskDecision::NoGo {
                reason: "exposure_limit_exceeded".into(),
            };
        }
        
        // Check 4: Confidence threshold
        let confidence = match &event.signal_type {
            super::ta_signal::SignalType::Buy { confidence, .. } => *confidence,
            super::ta_signal::SignalType::Sell { confidence, .. } => *confidence,
            _ => 1.0,
        };
        
        if confidence < 0.8 {
            self.orders_rejected.fetch_add(1, Ordering::Relaxed);
            return RiskDecision::NoGo {
                reason: "low_confidence".into(),
            };
        }
        
        // APPROVED
        let (side, quantity) = match &event.signal_type {
            super::ta_signal::SignalType::Buy { .. } => (OrderSide::Buy, 0.01),
            super::ta_signal::SignalType::Sell { .. } => (OrderSide::Sell, 0.01),
            _ => (OrderSide::Buy, 0.0),
        };
        
        self.orders_approved.fetch_add(1, Ordering::Relaxed);
        
        RiskDecision::Go {
            instrument,
            side,
            quantity,
            max_price: None,
        }
    }
    
    /// START THE SAGA - publish SagaStarted event
    fn start_order_saga(&mut self, decision: RiskDecision, signal: &super::ta_signal::SignalEvent) {
        let RiskDecision::Go { instrument, side, quantity, max_price } = decision else {
            return; // NoGo, don't start saga
        };
        
        let now = self.now_millis();
        
        // Generate saga ID
        static SAGA_COUNTER: AtomicU64 = AtomicU64::new(1);
        let saga_id = SagaId::new(SAGA_COUNTER.fetch_add(1, Ordering::Relaxed));
        
        // Block this instrument
        self.blocked_instruments.insert(instrument.clone());
        
        // Build saga context
        let context = SagaContext {
            saga_id,
            saga_type: "deribit_order".into(),
            step_name: "risk_approved".into(),
            correlation_id: saga_id.0,
            causation_id: 0,
            trace_id: saga_id.0,
            step_index: 0,
            attempt: 0,
            initiator_peer_id: [0u8; 32], // Local
            saga_started_at_millis: now,
            event_timestamp_millis: now,
        };
        
        // Build saga payload
        let payload = DeribitOrderPayload {
            instrument: instrument.clone(),
            side: match side {
                OrderSide::Buy => "buy",
                OrderSide::Sell => "sell",
            }.into(),
            quantity,
            order_type: "limit".into(),
            price: max_price,
            signal_timestamp: signal.timestamp,
            metadata: signal.metadata.clone(),
        };
        
        // Store saga state locally (we're the initiator)
        self.active_sagas.insert(saga_id, OrderSagaState::Started {
            instrument: instrument.clone(),
            started_at: now,
        });
        
        // Persist
        let _ = self.saga_journal.append(saga_id, ParticipantEvent::SagaRegistered {
            saga_type: "deribit_order".into(),
            step_name: "risk_approved".into(),
            registered_at_millis: now,
        });
        
        // PUBLISH SagaStarted - this triggers all participants
        let event = SagaChoreographyEvent::SagaStarted {
            context: context.clone(),
            payload: bincode::serialize(&payload).unwrap_or_default(),
        };
        
        self.saga_pubsub.publish("saga:deribit_order", event);
        
        tracing::info!(
            saga_id = %saga_id,
            instrument = %instrument,
            side = ?side,
            quantity = quantity,
            "Saga started: risk approved order"
        );
    }
    
    /// Handle saga completion - unblock instrument
    fn on_saga_completed(&mut self, context: &SagaContext) {
        if let Some(state) = self.active_sagas.remove(&context.saga_id) {
            if let OrderSagaState::Started { instrument, .. } = state {
                self.blocked_instruments.remove(&instrument);
                tracing::info!(
                    saga_id = %context.saga_id,
                    instrument = %instrument,
                    "Saga completed: instrument unblocked"
                );
            }
        }
    }
    
    /// Handle saga failure - unblock instrument
    fn on_saga_failed(&mut self, context: &SagaContext, reason: &str) {
        if let Some(state) = self.active_sagas.remove(&context.saga_id) {
            if let OrderSagaState::Started { instrument, .. } = state {
                self.blocked_instruments.remove(&instrument);
                tracing::warn!(
                    saga_id = %context.saga_id,
                    instrument = %instrument,
                    reason = %reason,
                    "Saga failed: instrument unblocked"
                );
            }
        }
    }
}

impl Actor for RiskManagerActor {
    type Msg = RiskManagerCommand;
    
    fn handle(&mut self, msg: Self::Msg) {
        match msg {
            RiskManagerCommand::OnSignal { event } => {
                // Evaluate risk
                let decision = self.evaluate_signal(&event);
                
                match &decision {
                    RiskDecision::Go { .. } => {
                        // START THE SAGA
                        self.start_order_saga(decision, &event);
                    }
                    RiskDecision::NoGo { reason } => {
                        tracing::debug!(
                            reason = %reason,
                            "Signal rejected"
                        );
                    }
                }
            }
            
            RiskManagerCommand::IsBlocked { instrument, reply_to } => {
                let blocked = self.blocked_instruments.contains(&instrument);
                let _ = reply_tell(reply_to, blocked);
            }
            
            RiskManagerCommand::GetMetrics { reply_to } => {
                let metrics = RiskMetrics {
                    orders_in_flight: self.blocked_instruments.len(),
                    total_exposure: self.total_exposure,
                    orders_rejected: self.orders_rejected.load(Ordering::Relaxed),
                    orders_approved: self.orders_approved.load(Ordering::Relaxed),
                };
                let _ = reply_tell(reply_to, metrics);
            }
            
            RiskManagerCommand::SagaEvent { event } => {
                // Handle saga lifecycle events
                match &event {
                    SagaChoreographyEvent::SagaCompleted { context } => {
                        self.on_saga_completed(context);
                    }
                    SagaChoreographyEvent::SagaFailed { context, reason } => {
                        self.on_saga_failed(context, reason);
                    }
                    SagaChoreographyEvent::SagaQuarantined { context, reason, .. } => {
                        self.on_saga_failed(context, reason);
                    }
                    _ => {}
                }
            }
            
            RiskManagerCommand::ClearBlock { instrument } => {
                self.blocked_instruments.remove(&instrument);
            }
        }
    }
}
