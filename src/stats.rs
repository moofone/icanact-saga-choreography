//! Participant statistics

use std::sync::atomic::{AtomicU64, Ordering};

/// Per-participant statistics
pub struct ParticipantStats {
    pub events_received: AtomicU64,
    pub events_relevant: AtomicU64,
    pub duplicate_events: AtomicU64,
    pub steps_started: AtomicU64,
    pub steps_completed: AtomicU64,
    pub steps_failed: AtomicU64,
    pub compensations_started: AtomicU64,
    pub compensations_completed: AtomicU64,
    pub quarantined_sagas: AtomicU64,
}

impl ParticipantStats {
    pub fn new() -> Self {
        Self {
            events_received: AtomicU64::new(0),
            events_relevant: AtomicU64::new(0),
            duplicate_events: AtomicU64::new(0),
            steps_started: AtomicU64::new(0),
            steps_completed: AtomicU64::new(0),
            steps_failed: AtomicU64::new(0),
            compensations_started: AtomicU64::new(0),
            compensations_completed: AtomicU64::new(0),
            quarantined_sagas: AtomicU64::new(0),
        }
    }
    
    pub fn snapshot(&self) -> ParticipantStatsSnapshot {
        ParticipantStatsSnapshot {
            events_received: self.events_received.load(Ordering::Relaxed),
            events_relevant: self.events_relevant.load(Ordering::Relaxed),
            duplicate_events: self.duplicate_events.load(Ordering::Relaxed),
            steps_started: self.steps_started.load(Ordering::Relaxed),
            steps_completed: self.steps_completed.load(Ordering::Relaxed),
            steps_failed: self.steps_failed.load(Ordering::Relaxed),
            compensations_started: self.compensations_started.load(Ordering::Relaxed),
            compensations_completed: self.compensations_completed.load(Ordering::Relaxed),
            quarantined_sagas: self.quarantined_sagas.load(Ordering::Relaxed),
        }
    }
}

impl Default for ParticipantStats {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug)]
pub struct ParticipantStatsSnapshot {
    pub events_received: u64,
    pub events_relevant: u64,
    pub duplicate_events: u64,
    pub steps_started: u64,
    pub steps_completed: u64,
    pub steps_failed: u64,
    pub compensations_started: u64,
    pub compensations_completed: u64,
    pub quarantined_sagas: u64,
}
