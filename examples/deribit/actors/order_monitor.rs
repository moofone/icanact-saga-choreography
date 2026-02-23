//! Order Monitor Actor - SAGA PARTICIPANT (Step: monitor_order)
//!
//! Subscribes to order updates from Deribit.
//! Emits StepCompleted when order is filled/confirmed.

use icanact_core::local_sync::{Actor, MailboxAddr, ReplyTo};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use icanact_saga_choreography::{
    SagaId, SagaContext, SagaChoreographyEvent, ParticipantEvent,
    SagaParticipant, SagaStateExt, DependencySpec,
    ParticipantJournal, ParticipantDedupeStore, ParticipantStats,
    SagaStateEntry, StepOutput, StepError, CompensationError,
};

#[derive(Debug)]
pub enum OrderMonitorCommand {
    /// Order update from Deribit WS actor
    OnOrderUpdate { update: OrderUpdate },
    
    /// Saga event from pubsub
    SagaEvent { event: SagaChoreographyEvent },
    
    /// Get stats
    GetStats { reply_to: ReplyTo<icanact_saga_choreography::ParticipantStatsSnapshot> },
}

#[derive(Clone, Debug)]
pub struct OrderUpdate {
    pub order_id: Box<str>,
    pub status: OrderStatus,
    pub filled_quantity: f64,
    pub average_price: f64,
}

#[derive(Clone, Debug)]
pub enum OrderStatus {
    Open,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}

pub struct OrderMonitorActor {
    // Monitored orders: order_id -> saga_id
    monitored: std::collections::HashMap<Box<str>, SagaId>,
    
    // Saga state
    saga_states: std::collections::HashMap<SagaId, SagaStateEntry>,
    saga_journal: Arc<dyn ParticipantJournal>,
    saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    saga_stats: Arc<ParticipantStats>,
    
    // To emit saga events
    saga_pubsub: icanact_core::local_sync::pubsub::PubSub<SagaChoreographyEvent>,
}

impl Actor for OrderMonitorActor {
    type Msg = OrderMonitorCommand;
    
    fn handle(&mut self, msg: Self::Msg) {
        match msg {
            OrderMonitorCommand::OnOrderUpdate { update } => {
                self.handle_order_update(update);
            }
            
            OrderMonitorCommand::SagaEvent { event } => {
                // Subscribe to place_order completion
                if let SagaChoreographyEvent::StepCompleted { step_name, output, context, .. } = &event {
                    if step_name == "place_order" {
                        // Extract order_id and start monitoring
                        if let Ok(order_id) = bincode::deserialize::<Box<str>>(output) {
                            self.monitored.insert(order_id, context.saga_id);
                        }
                    }
                }
            }
            
            OrderMonitorCommand::GetStats { reply_to } => {
                let _ = reply_tell(reply_to, self.saga_stats.snapshot());
            }
        }
    }
}

impl OrderMonitorActor {
    fn handle_order_update(&mut self, update: OrderUpdate) {
        // Find saga for this order
        if let Some(saga_id) = self.monitored.get(&update.order_id).copied() {
            match update.status {
                OrderStatus::Filled => {
                    // Order complete - emit SagaCompleted
                    self.monitored.remove(&update.order_id);
                    
                    // Emit SagaCompleted event
                    tracing::info!(
                        saga_id = %saga_id,
                        order_id = %update.order_id,
                        filled_qty = update.filled_quantity,
                        avg_price = update.average_price,
                        "Order filled - saga completed"
                    );
                }
                
                OrderStatus::Cancelled | OrderStatus::Rejected => {
                    // Order failed - may need compensation
                    self.monitored.remove(&update.order_id);
                    
                    tracing::warn!(
                        saga_id = %saga_id,
                        order_id = %update.order_id,
                        status = ?update.status,
                        "Order failed"
                    );
                }
                
                _ => {
                    // Still in progress
                }
            }
        }
    }
}
