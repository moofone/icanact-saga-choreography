//! Saga events

use serde::{Deserialize, Serialize};
use super::{SagaContext, SagaId};

/// Events published via DistributedPubSub
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SagaChoreographyEvent {
    // Saga lifecycle
    SagaStarted { context: SagaContext, payload: Vec<u8> },
    SagaCompleted { context: SagaContext },
    SagaFailed { context: SagaContext, reason: Box<str> },
    
    // Step execution
    StepStarted { context: SagaContext },
    StepCompleted {
        context: SagaContext,
        output: Vec<u8>,
        compensation_available: bool,
    },
    StepFailed {
        context: SagaContext,
        error: Box<str>,
        will_retry: bool,
        requires_compensation: bool,
    },
    
    // Compensation
    CompensationRequested {
        context: SagaContext,
        failed_step: Box<str>,
        reason: Box<str>,
        steps_to_compensate: Vec<Box<str>>,
    },
    CompensationStarted { context: SagaContext },
    CompensationCompleted { context: SagaContext },
    CompensationFailed {
        context: SagaContext,
        error: Box<str>,
        is_ambiguous: bool,
    },
    SagaQuarantined {
        context: SagaContext,
        reason: Box<str>,
        step: Box<str>,
    },
    
    // Acknowledgment
    StepAck {
        context: SagaContext,
        participant_id: super::PeerId,
        status: AckStatus,
    },
}

impl SagaChoreographyEvent {
    pub fn context(&self) -> &SagaContext {
        match self {
            Self::SagaStarted { context, .. } => context,
            Self::SagaCompleted { context } => context,
            Self::SagaFailed { context, .. } => context,
            Self::StepStarted { context } => context,
            Self::StepCompleted { context, .. } => context,
            Self::StepFailed { context, .. } => context,
            Self::CompensationRequested { context, .. } => context,
            Self::CompensationStarted { context } => context,
            Self::CompensationCompleted { context } => context,
            Self::CompensationFailed { context, .. } => context,
            Self::SagaQuarantined { context, .. } => context,
            Self::StepAck { context, .. } => context,
        }
    }
    
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::SagaStarted { .. } => "saga_started",
            Self::SagaCompleted { .. } => "saga_completed",
            Self::SagaFailed { .. } => "saga_failed",
            Self::StepStarted { .. } => "step_started",
            Self::StepCompleted { .. } => "step_completed",
            Self::StepFailed { .. } => "step_failed",
            Self::CompensationRequested { .. } => "compensation_requested",
            Self::CompensationStarted { .. } => "compensation_started",
            Self::CompensationCompleted { .. } => "compensation_completed",
            Self::CompensationFailed { .. } => "compensation_failed",
            Self::SagaQuarantined { .. } => "saga_quarantined",
            Self::StepAck { .. } => "step_ack",
        }
    }
    
    pub fn topic(saga_type: &str) -> String {
        format!("saga:{}", saga_type)
    }
}

/// Acknowledgment status
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AckStatus {
    Accepted,
    Completed,
    Failed,
    NotApplicable,
    AlreadyProcessing,
}

/// Events stored in participant's local journal
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ParticipantEvent {
    SagaRegistered { saga_type: Box<str>, step_name: Box<str>, registered_at_millis: u64 },
    StepTriggered { triggering_event: Box<str>, triggered_at_millis: u64 },
    StepExecutionStarted { attempt: u32, started_at_millis: u64 },
    StepExecutionCompleted { output: Vec<u8>, compensation_data: Vec<u8>, completed_at_millis: u64 },
    StepExecutionFailed { error: Box<str>, requires_compensation: bool, failed_at_millis: u64 },
    CompensationStarted { attempt: u32, started_at_millis: u64 },
    CompensationCompleted { completed_at_millis: u64 },
    CompensationFailed { error: Box<str>, is_ambiguous: bool, failed_at_millis: u64 },
    Quarantined { reason: Box<str>, quarantined_at_millis: u64 },
}
