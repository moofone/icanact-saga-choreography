//! TA Signal Actor - emits trading signals
//!
//! This is a SYNC LOCAL actor for maximum speed.
//! It analyzes market data and emits go/no-go signals.

use icanact_core::local_sync::{Actor, MailboxAddr, ReplyTo};
use std::collections::VecDeque;

/// Trading signal type
#[derive(Clone, Debug)]
pub enum SignalType {
    /// Buy signal with confidence (0.0 - 1.0)
    Buy { instrument: Box<str>, confidence: f64 },
    /// Sell signal with confidence
    Sell { instrument: Box<str>, confidence: f64 },
    /// Exit position
    Exit { instrument: Box<str>,
    order_id: Box<str> },
}

/// TA signal actor command
#[derive(Debug)]
pub enum TASignalCommand {
    /// Market data update (trigger for signal generation)
    OnMarketData { 
        instrument: Box<str>, 
        price: f64, 
        volume: f64,
        timestamp: u64,
    },
    
    /// Subscribe to signals
    Subscribe { 
        subscriber: MailboxAddr<SignalEvent>,
    },
    
    /// Get current signal
    GetCurrentSignal { 
        instrument: Box<str>,
        reply_to: ReplyTo<Option<SignalType>>,
    },
    
    /// Enable/disable signal generation
    SetEnabled { enabled: bool },
}

/// Signal event published to subscribers
#[derive(Clone, Debug)]
pub struct SignalEvent {
    pub signal_type: SignalType,
    pub timestamp: u64,
    pub metadata: Vec<(Box<str, Box<str>)>,
}

/// TA Signal Actor - sync local for speed
pub struct TASignalActor {
    // Subscribers to signal events
    subscribers: Vec<MailboxAddr<SignalEvent>>,
    
    // Current signals per instrument
    current_signals: std::collections::HashMap<Box<str>, SignalType>,
    
    // Market data buffer for analysis
    price_history: std::collections::HashMap<Box<str>, VecDeque<f64>>,
    
    // Configuration
    enabled: bool,
    signal_threshold: f64,
    history_size: usize,
    
    // Dependencies
    risk_manager: MailboxAddr<super::RiskManagerCommand>,
}

impl TASignalActor {
    pub fn new(
        risk_manager: MailboxAddr<super::RiskManagerCommand>,
    ) -> Self {
        Self {
            subscribers: Vec::new(),
            current_signals: std::collections::HashMap::new(),
            price_history: std::collections::HashMap::new(),
            enabled: true,
            signal_threshold: 0.7,
            history_size: 100,
            risk_manager,
        }
    }
    
    fn analyze_and_generate_signal(&mut self, instrument: &str, price: f64) -> Option<SignalType> {
        // Simple example: momentum-based signal
        let history = self.price_history.entry(instrument.into()).or_default();
        history.push_back(price);
        if history.len() > self.history_size {
            history.pop_front();
        }
        
        if history.len() < 10 {
            return None;
        }
        
        // Calculate simple momentum
        let recent: f64 = history.iter().rev().take(5).sum::<f64>() / 5.0;
        let older: f64 = history.iter().rev().skip(5).take(5).sum::<f64>() / 5.0;
        
        let momentum = (recent - older) / older;
        let confidence = momentum.abs().min(1.0);
        
        if confidence >= self.signal_threshold {
            if momentum > 0.0 {
                Some(SignalType::Buy {
                    instrument: instrument.into(),
                    confidence,
                })
            } else {
                Some(SignalType::Sell {
                    instrument: instrument.into(),
                    confidence,
                })
            }
        } else {
            None
        }
    }
    
    fn emit_signal(&self, event: SignalEvent) {
        for subscriber in &self.subscribers {
            let _ = subscriber.try_tell(event.clone());
        }
    }
}

impl Actor for TASignalActor {
    type Msg = TASignalCommand;
    
    fn handle(&mut self, msg: Self::Msg) {
        match msg {
            TASignalCommand::OnMarketData { instrument, price, timestamp, .. } => {
                if !self.enabled {
                    return;
                }
                
                // Analyze and potentially generate signal
                if let Some(signal) = self.analyze_and_generate_signal(&instrument, price) {
                    // Store current signal
                    self.current_signals.insert(instrument.clone(), signal.clone());
                    
                    // Emit to subscribers (includes RiskManager)
                    let event = SignalEvent {
                        signal_type: signal,
                        timestamp,
                        metadata: vec![("source".into(), "ta_momentum".into())],
                    };
                    self.emit_signal(event);
                }
            }
            
            TASignalCommand::Subscribe { subscriber } => {
                self.subscribers.push(subscriber);
            }
            
            TASignalCommand::GetCurrentSignal { instrument, reply_to } => {
                let _ = reply_tell(reply_to, self.current_signals.get(&instrument).cloned());
            }
            
            TASignalCommand::SetEnabled { enabled } => {
                self.enabled = enabled;
            }
        }
    }
}
