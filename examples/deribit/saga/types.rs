//! Deribit Order Saga Type Definitions

use serde::{Deserialize, Serialize};

/// Payload for deribit_order saga
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DeribitOrderPayload {
    pub instrument: Box<str>,
    pub side: Box<str>,
    pub quantity: f64,
    pub order_type: Box<str>,
    pub price: Option<f64>,
    pub signal_timestamp: u64,
    pub metadata: Vec<(Box<str, Box<str>)>,
}

/// Saga state tracked by initiator (RiskManager)
#[derive(Clone, Debug)]
pub enum OrderSagaState {
    Started {
        instrument: Box<str>,
        started_at: u64,
    },
    OrderPlaced {
        instrument: Box<str>,
        order_id: Box<str>,
        started_at: u64,
        placed_at: u64,
    },
    Completed {
        instrument: Box<str>,
        order_id: Box<str>,
        filled_quantity: f64,
        average_price: f64,
        started_at: u64,
        completed_at: u64,
    },
    Failed {
        instrument: Box<str>,
        reason: Box<str>,
        started_at: u64,
        failed_at: u64,
    },
    Compensated {
        instrument: Box<str>,
        order_id: Box<str>,
        reason: Box<str>,
        started_at: u64,
        compensated_at: u64,
    },
}

/// Steps in the deribit_order saga
pub mod steps {
    pub const RISK_APPROVED: &str = "risk_approved";
    pub const PREPARE_ORDER: &str = "prepare_order";
    pub const PLACE_ORDER: &str = "place_order";
    pub const MONITOR_ORDER: &str = "monitor_order";
}

/// Saga type identifier
pub const SAGA_TYPE: &str = "deribit_order";
