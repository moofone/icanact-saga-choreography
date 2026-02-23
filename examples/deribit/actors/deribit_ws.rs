//! Deribit WS Actor - ASYNC actor, owns WebSocket connection
//!
//! NOT a saga participant - just handles WS communication.
//! Other actors call this via ask to place/cancel orders.

use icanact_core::local_async::Actor;
use icanact_core::local_sync::ReplyTo;
use async_trait::async_trait;

#[derive(Debug)]
pub enum DeribitWSCommand {
    /// Place an order
    PlaceOrder {
        instrument: Box<str>,
        side: Box<str>,
        quantity: f64,
        order_type: Box<str>,
        price: Option<f64>,
        client_id: Box<str>,
        idempotency_key: Box<str>,
        post_only: bool,
        reduce_only: bool,
        reply_to: ReplyTo<DeribitWSResponse>,
    },
    
    /// Cancel an order
    CancelOrder {
        order_id: Box<str>,
        reply_to: ReplyTo<DeribitWSResponse>,
    },
    
    /// Modify an order
    ModifyOrder {
        order_id: Box<str>,
        new_quantity: Option<f64>,
        new_price: Option<f64>,
        reply_to: ReplyTo<DeribitWSResponse>,
    },
    
    /// Subscribe to order updates
    SubscribeOrders {
        reply_to: ReplyTo<Result<(), Box<str>>>,
    },
    
    /// Connection management
    Connect { reply_to: ReplyTo<Result<(), Box<str>>> },
    Disconnect,
}

#[derive(Clone, Debug)]
pub enum DeribitWSResponse {
    OrderPlaced { order_id: Box<str> },
    OrderCancelled,
    OrderModified,
    OrderRejected { reason: Box<str> },
    Error { message: Box<str> },
}

pub struct DeribitWSActor {
    // WS connection (would be actual WS client)
    connected: bool,
    
    // Subscribers to order updates
    order_subscribers: Vec<icanact_core::local_sync::MailboxAddr<super::OrderMonitorCommand>>,
}

impl DeribitWSActor {
    pub fn new() -> Self {
        Self {
            connected: false,
            order_subscribers: Vec::new(),
        }
    }
}

#[async_trait]
impl Actor for DeribitWSActor {
    type Msg = DeribitWSCommand;
    
    async fn handle(&mut self, msg: Self::Msg) {
        match msg {
            DeribitWSCommand::Connect { reply_to } => {
                // TODO: Actual WS connection
                self.connected = true;
                let _ = reply_tell(reply_to, Ok(()));
            }
            
            DeribitWSCommand::Disconnect => {
                self.connected = false;
            }
            
            DeribitWSCommand::PlaceOrder {
                instrument, side, quantity, order_type, price,
                client_id, idempotency_key, post_only, reduce_only,
                reply_to,
            } => {
                if !self.connected {
                    let _ = reply_tell(reply_to, DeribitWSResponse::Error {
                        message: "not_connected".into(),
                    });
                    return;
                }
                
                // TODO: Send to Deribit via WS
                // For now, simulate success
                let order_id = format!("order-{}", uuid::Uuid::new_v4());
                
                tracing::info!(
                    instrument = %instrument,
                    side = %side,
                    quantity = quantity,
                    client_id = %client_id,
                    "Order placed"
                );
                
                let _ = reply_tell(reply_to, DeribitWSResponse::OrderPlaced {
                    order_id: order_id.into(),
                });
            }
            
            DeribitWSCommand::CancelOrder { order_id, reply_to } => {
                if !self.connected {
                    let _ = reply_tell(reply_to, DeribitWSResponse::Error {
                        message: "not_connected".into(),
                    });
                    return;
                }
                
                // TODO: Send cancel to Deribit
                tracing::info!(order_id = %order_id, "Order cancelled");
                
                let _ = reply_tell(reply_to, DeribitWSResponse::OrderCancelled);
            }
            
            DeribitWSCommand::SubscribeOrders { reply_to } => {
                let _ = reply_tell(reply_to, Ok(()));
            }
            
            _ => {}
        }
    }
}
