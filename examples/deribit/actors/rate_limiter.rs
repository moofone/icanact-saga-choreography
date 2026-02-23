//! Rate Limiter Actor - NOT a saga participant
//!
//! Simple sync actor that tracks rate limits.
//! Called by RiskManager before approving orders.

use icanact_core::local_sync::{Actor, ReplyTo};
use std::collections::VecDeque;
use std::time::Duration;

#[derive(Debug)]
pub enum RateLimiterCommand {
    CheckAllowed {
        key: Box<str>,
        reply_to: ReplyTo<RateLimitResult>,
    },
    Reset { key: Box<str> },
}

#[derive(Clone, Debug)]
pub enum RateLimitResult {
    Allowed,
    Denied { retry_after_millis: u64 },
}

pub struct RateLimiterActor {
    // Per-key timestamps
    requests: std::collections::HashMap<Box<str>, VecDeque<u64>>,
    
    // Limits
    max_requests: u32,
    window_millis: u64,
    
    // Time
    clock: fn() -> u64,
}

impl RateLimiterActor {
    pub fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            requests: std::collections::HashMap::new(),
            max_requests,
            window_millis: window.as_millis() as u64,
            clock: || std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }
    
    fn prune_window(&mut self, key: &str, now: u64) {
        if let Some(timestamps) = self.requests.get_mut(key) {
            let cutoff = now.saturating_sub(self.window_millis);
            while timestamps.front().map_or(false, |&t| t < cutoff) {
                timestamps.pop_front();
            }
        }
    }
}

impl Actor for RateLimiterActor {
    type Msg = RateLimiterCommand;
    
    fn handle(&mut self, msg: Self::Msg) {
        match msg {
            RateLimiterCommand::CheckAllowed { key, reply_to } => {
                let now = (self.clock)();
                self.prune_window(&key, now);
                
                let timestamps = self.requests.entry(key).or_default();
                let count = timestamps.len() as u32;
                
                if count < self.max_requests {
                    timestamps.push_back(now);
                    let _ = reply_tell(reply_to, RateLimitResult::Allowed);
                } else {
                    let oldest = timestamps.front().copied().unwrap_or(now);
                    let retry_after = oldest + self.window_millis - now;
                    let _ = reply_tell(reply_to, RateLimitResult::Denied {
                        retry_after_millis: retry_after,
                    });
                }
            }
            
            RateLimiterCommand::Reset { key } => {
                self.requests.remove(&key);
            }
        }
    }
}
