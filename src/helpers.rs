//! Helper functions for saga handling

use crate::{
    Compensating, CompensationError, Completed, DependencySpec, Executing, ParticipantEvent,
    SagaChoreographyEvent, SagaContext, SagaId, SagaParticipant, SagaParticipantState,
    SagaStateEntry, SagaStateExt, StepError, StepOutput,
};
use std::sync::atomic::Ordering;

/// Default saga event handler.
///
/// Call this from your actor's `handle` method when a `SagaEvent` message arrives.
///
/// ```rust,ignore
/// impl Actor for MyActor {
///     type Msg = MyCommand;
///     fn handle(&mut self, msg: Self::Msg) {
///         match msg {
///             MyCommand::SagaEvent { event } => {
///                 handle_saga_event(self, event);
///             }
///             // ...
///         }
///     }
/// }
/// ```
pub fn handle_saga_event<P>(participant: &mut P, event: SagaChoreographyEvent)
where
    P: SagaParticipant + SagaStateExt,
{
    let context = event.context().clone();
    let now = participant.now_millis();

    // Check saga type
    if !participant
        .saga_types()
        .iter()
        .any(|t| *t == context.saga_type.as_ref())
    {
        return;
    }

    // Idempotency check
    let dedupe_key = format!("{}:{}", context.trace_id, event.event_type());
    if !participant.check_dedupe(context.saga_id, &dedupe_key) {
        return; // Already processed
    }

    match event {
        SagaChoreographyEvent::SagaStarted { payload, .. }
            if participant.depends_on().is_on_saga_start() =>
        {
            execute_step_wrapper(participant, context.clone(), payload, now);
        }

        SagaChoreographyEvent::StepCompleted {
            context: step_ctx,
            output,
            ..
        } => {
            if participant
                .depends_on()
                .is_satisfied_by(&step_ctx.step_name)
            {
                let next_context = context.next_step(participant.step_name().into());
                execute_step_wrapper(participant, next_context, output, now);
            }
        }

        SagaChoreographyEvent::CompensationRequested {
            steps_to_compensate,
            ..
        } => {
            if steps_to_compensate.contains(&participant.step_name().into()) {
                compensate_wrapper(participant, &context, now);
            }
        }

        SagaChoreographyEvent::SagaCompleted { .. } => {
            participant.on_saga_completed(&context);
            participant.prune_saga(context.saga_id);
        }

        SagaChoreographyEvent::SagaFailed { reason, .. } => {
            participant.on_saga_failed(&context, &reason);
            participant.prune_saga(context.saga_id);
        }

        _ => {}
    }
}

/// Execute a step with full state management
pub fn execute_step_wrapper<P>(participant: &mut P, context: SagaContext, input: Vec<u8>, now: u64)
where
    P: SagaParticipant + SagaStateExt,
{
    let saga_id = context.saga_id;

    // Build state: Idle -> Triggered -> Executing
    let state = SagaParticipantState::new(
        saga_id,
        context.saga_type.clone(),
        participant.step_name().into(),
        context.correlation_id,
        context.trace_id,
        context.initiator_peer_id,
        context.saga_started_at_millis,
    )
    .trigger("dependency_satisfied", now)
    .start_execution(now);

    // Persist
    participant.record_event(
        saga_id,
        ParticipantEvent::StepExecutionStarted {
            attempt: 1,
            started_at_millis: now,
        },
    );

    // Store state
    participant
        .saga_states()
        .insert(saga_id, SagaStateEntry::Executing(state));

    // Execute
    match participant.execute_step(&context, &input) {
        Ok(output) => {
            complete_step(participant, &context, output, now);
        }
        Err(error) => {
            fail_step(participant, &context, error, now);
        }
    }
}

/// Complete a step with state transition
fn complete_step<P>(participant: &mut P, context: &SagaContext, output: StepOutput, now: u64)
where
    P: SagaParticipant + SagaStateExt,
{
    let saga_id = context.saga_id;
    let (out_data, comp_data) = match output {
        StepOutput::Completed {
            output,
            compensation_data,
        } => (output, compensation_data),
        StepOutput::CompletedWithEffect {
            output,
            compensation_data,
            ..
        } => (output, compensation_data),
    };

    // State: Executing -> Completed
    if let Some(SagaStateEntry::Executing(state)) = participant.saga_states().remove(&saga_id) {
        let new_state = state.complete(out_data.clone(), comp_data, now);
        participant
            .saga_states()
            .insert(saga_id, SagaStateEntry::Completed(new_state));
    }

    // Persist
    participant.record_event(
        saga_id,
        ParticipantEvent::StepExecutionCompleted {
            output: out_data,
            compensation_data: vec![],
            completed_at_millis: now,
        },
    );
}

/// Fail a step with state transition
fn fail_step<P>(participant: &mut P, context: &SagaContext, error: StepError, now: u64)
where
    P: SagaParticipant + SagaStateExt,
{
    let saga_id = context.saga_id;
    let (reason, requires_comp) = match error {
        StepError::Retriable { reason } => {
            // TODO: Handle retry with backoff
            return;
        }
        StepError::Terminal { reason } => (reason, false),
        StepError::RequireCompensation { reason } => (reason, true),
    };

    // State: Executing -> Failed
    if let Some(SagaStateEntry::Executing(state)) = participant.saga_states().remove(&saga_id) {
        use crate::state::Failed;
        let new_state = state.fail(reason.clone(), requires_comp, now);
        participant
            .saga_states()
            .insert(saga_id, SagaStateEntry::Failed(new_state));
    }

    // Persist
    participant.record_event(
        saga_id,
        ParticipantEvent::StepExecutionFailed {
            error: reason,
            requires_compensation: requires_comp,
            failed_at_millis: now,
        },
    );
}

/// Execute compensation with state management
pub fn compensate_wrapper<P>(participant: &mut P, context: &SagaContext, now: u64)
where
    P: SagaParticipant + SagaStateExt,
{
    let saga_id = context.saga_id;

    // Get compensation data from Completed state
    if let Some(SagaStateEntry::Completed(state)) = participant.saga_states().remove(&saga_id) {
        let comp_data = state.state.compensation_data.clone();

        // State: Completed -> Compensating
        let new_state = state.start_compensation(now);
        participant
            .saga_states()
            .insert(saga_id, SagaStateEntry::Compensating(new_state));

        // Persist
        participant.record_event(
            saga_id,
            ParticipantEvent::CompensationStarted {
                attempt: 1,
                started_at_millis: now,
            },
        );

        // Execute compensation
        match participant.compensate_step(context, &comp_data) {
            Ok(()) => {
                complete_compensation(participant, context, now);
            }
            Err(error) => {
                fail_compensation(participant, context, error, now);
            }
        }
    }
}

/// Complete compensation
fn complete_compensation<P>(participant: &mut P, context: &SagaContext, now: u64)
where
    P: SagaParticipant + SagaStateExt,
{
    let saga_id = context.saga_id;

    // State: Compensating -> Compensated
    if let Some(SagaStateEntry::Compensating(state)) = participant.saga_states().remove(&saga_id) {
        use crate::state::Compensated;
        let new_state = state.complete_compensation(now);
        participant
            .saga_states()
            .insert(saga_id, SagaStateEntry::Compensated(new_state));
    }

    // Persist
    participant.record_event(
        saga_id,
        ParticipantEvent::CompensationCompleted {
            completed_at_millis: now,
        },
    );

    // Notify
    participant.on_compensation_completed(context);
}

/// Fail compensation (quarantine)
fn fail_compensation<P>(
    participant: &mut P,
    context: &SagaContext,
    error: CompensationError,
    now: u64,
) where
    P: SagaParticipant + SagaStateExt,
{
    let saga_id = context.saga_id;
    let reason = match error {
        CompensationError::SafeToRetry { reason } => reason,
        CompensationError::Ambiguous { reason } => reason,
        CompensationError::Terminal { reason } => reason,
    };

    // State: Compensating -> Quarantined
    if let Some(SagaStateEntry::Compensating(state)) = participant.saga_states().remove(&saga_id) {
        use crate::state::Quarantined;
        let new_state = state.quarantine(reason.clone(), now);
        participant
            .saga_states()
            .insert(saga_id, SagaStateEntry::Quarantined(new_state));
    }

    // Persist
    participant.record_event(
        saga_id,
        ParticipantEvent::Quarantined {
            reason: reason.clone(),
            quarantined_at_millis: now,
        },
    );

    // Notify
    participant.on_quarantined(context, &reason);
}

/// Recovery bootstrap - find and resume pending sagas
pub fn recover_sagas<P>(participant: &mut P) -> Vec<SagaId>
where
    P: SagaParticipant + SagaStateExt,
{
    let mut recovered = Vec::new();

    if let Ok(saga_ids) = participant.saga_journal().list_sagas() {
        for saga_id in saga_ids {
            if let Ok(events) = participant.saga_journal().read(saga_id) {
                let state = rebuild_state(&events);

                if !state.is_terminal() {
                    recovered.push(saga_id);
                    // TODO: Resume based on state
                }
            }
        }
    }

    recovered
}

/// Rebuild state from event history
fn rebuild_state(entries: &[crate::JournalEntry]) -> RebuiltState {
    let mut state = RebuiltState::Unknown;

    for entry in entries {
        state = match (state, &entry.event) {
            (_, ParticipantEvent::StepExecutionStarted { .. }) => RebuiltState::Executing,
            (_, ParticipantEvent::StepExecutionCompleted { .. }) => RebuiltState::Completed,
            (
                _,
                ParticipantEvent::StepExecutionFailed {
                    requires_compensation: true,
                    ..
                },
            ) => RebuiltState::FailedNeedsCompensation,
            (
                _,
                ParticipantEvent::StepExecutionFailed {
                    requires_compensation: false,
                    ..
                },
            ) => RebuiltState::FailedTerminal,
            (_, ParticipantEvent::CompensationStarted { .. }) => RebuiltState::Compensating,
            (_, ParticipantEvent::CompensationCompleted { .. }) => RebuiltState::Compensated,
            (_, ParticipantEvent::Quarantined { .. }) => RebuiltState::Quarantined,
            _ => state,
        };
    }

    state
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RebuiltState {
    Unknown,
    Executing,
    Completed,
    FailedNeedsCompensation,
    FailedTerminal,
    Compensating,
    Compensated,
    Quarantined,
}

impl RebuiltState {
    fn is_terminal(&self) -> bool {
        matches!(
            self,
            RebuiltState::Completed
                | RebuiltState::FailedTerminal
                | RebuiltState::Compensated
                | RebuiltState::Quarantined
        )
    }
}
