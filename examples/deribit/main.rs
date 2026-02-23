//! Deribit Order Workflow Example
//!
//! Demonstrates choreography-based saga for placing orders on Deribit.
//!
//! ## Actors
//!
//! - `TASignalActor`: Analyzes market data, emits signals (sync)
//! - `RiskManagerActor`: Evaluates risk, starts sagas on GO (sync)
//! - `OrderPlacerActor`: Prepares order payload (sync)
//! - `OrderCoordinatorActor`: Sends order to Deribit (sync)
//! - `DeribitWSActor`: WebSocket connection to Deribit (async)
//! - `OrderMonitorActor`: Monitors order status (sync)
//! - `RateLimiterActor`: Rate limiting (sync)

mod actors;
mod saga;

use actors::*;
use icanact_core::local_sync::{
    Actor as SyncActor, 
    MailboxAddr, 
    pubsub::PubSub,
    supervisor::{SupervisorBuilder, Restart},
};
use icanact_saga_choreography::{
    ParticipantJournal, ParticipantDedupeStore,
    InMemoryJournal, InMemoryDedupe,
    SagaChoreographyEvent,
};
use std::sync::Arc;

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();
    
    tracing::info!("Starting Deribit Order Workflow Example");
    
    // Create saga pubsub
    let saga_pubsub: PubSub<SagaChoreographyEvent> = PubSub::new();
    
    // Create storage (in-memory for example, would use Heed in production)
    let journal: Arc<dyn ParticipantJournal> = Arc::new(InMemoryJournal::new());
    let dedupe: Arc<dyn ParticipantDedupeStore> = Arc::new(InMemoryDedupe::new());
    
    // Build supervisor
    let mut builder = SupervisorBuilder::new(
        icanact_core::local_sync::supervisor::Strategy::OneForOne,
        icanact_core::local_sync::supervisor::RestartIntensity {
            max_restarts: 3,
            within: std::time::Duration::from_secs(60),
        },
    );
    
    // Create RateLimiter
    let rate_limiter = builder.add_child(
        "rate_limiter",
        100,
        Restart::Permanent,
        || RateLimiterActor::new(10, std::time::Duration::from_secs(60)),
    );
    
    // Create RiskManager (SAGA INITIATOR)
    let risk_manager = builder.add_child(
        "risk_manager",
        1000,
        Restart::Permanent,
        || {
            RiskManagerActor::new(
                rate_limiter.clone(),
                saga_pubsub.clone(),
                journal.clone(),
                dedupe.clone(),
            )
        },
    );
    
    // Subscribe RiskManager to saga events
    saga_pubsub.subscribe("saga:deribit_order", risk_manager.clone());
    
    // Create TASignal (emits signals to RiskManager)
    let _ta_signal = builder.add_child(
        "ta_signal",
        1000,
        Restart::Permanent,
        || TASignalActor::new(risk_manager.clone()),
    );
    
    // Spawn supervisor
    let (_supervisor_addr, _supervisor_handle) = builder.spawn(100);
    
    tracing::info!("All actors started");
    
    // In a real app, we'd now:
    // 1. Feed market data to TASignalActor
    // 2. TASignalActor emits signals
    // 3. RiskManager evaluates, starts sagas
    // 4. OrderPlacerActor prepares orders
    // 5. OrderCoordinatorActor sends to DeribitWSActor
    // 6. OrderMonitorActor tracks completion
    
    // For demo, just keep running
    tracing::info!("Running... press Ctrl+C to stop");
    
    // Simple run loop
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
