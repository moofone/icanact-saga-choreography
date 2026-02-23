# LLM Integration Guide

Use this when adding a new saga participant actor to an `icanact-core` app with `icanact-saga-choreography`.

## Required Actor Layout

Follow this structure from `spec/PLAN.md`:

```text
src/actors/<actor-name>/
├── mod.rs
├── actor.rs
├── messaging.rs
├── business.rs
├── saga/
│   ├── mod.rs
│   ├── participant.rs
│   ├── state_ext.rs
│   └── types.rs            # optional
├── tests/
│   ├── unit.rs
│   ├── integration.rs      # optional
│   └── saga.rs
└── architecture.md         # short role/flow notes
```

## Required Actor Fields

Your actor struct must include saga state + storage handles:

```rust
pub struct MyActor {
    // business state + dependencies...
    saga_states: HashMap<SagaId, SagaStateEntry>,
    saga_journal: Arc<dyn ParticipantJournal>,
    saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    saga_stats: Arc<ParticipantStats>,
}
```

## Required Traits

Implement exactly these two traits for each saga participant.

### 1) `SagaStateExt` (infrastructure access)

Implement all required methods:

```rust
impl SagaStateExt for MyActor {
    fn saga_states(&mut self) -> &mut HashMap<SagaId, SagaStateEntry> { &mut self.saga_states }
    fn saga_states_ref(&self) -> &HashMap<SagaId, SagaStateEntry> { &self.saga_states }
    fn saga_journal(&self) -> &Arc<dyn ParticipantJournal> { &self.saga_journal }
    fn saga_dedupe(&self) -> &Arc<dyn ParticipantDedupeStore> { &self.saga_dedupe }
    fn now_millis(&self) -> u64 { /* unix millis */ }
}
```

### 2) `SagaParticipant` (business behavior)

Implement required methods:

- `step_name()`
- `saga_types()`
- `execute_step(&SagaContext, &[u8]) -> Result<StepOutput, StepError>`
- `compensate_step(&SagaContext, &[u8]) -> Result<(), CompensationError>`

Use `depends_on()` for choreography ordering (`OnSagaStart`, `After`, `AnyOf`, `AllOf`).

Keep core business logic in `business.rs` as pure functions; `participant.rs` should mostly deserialize, call business logic, and map errors to `StepError`/`CompensationError`.

## Messaging + PubSub Wiring

In `messaging.rs`, add:

```rust
SagaEvent { event: SagaChoreographyEvent },
RecoverSagas { reply_to: ReplyTo<Vec<SagaId>> },
GetSagaStats { reply_to: ReplyTo<ParticipantStatsSnapshot> },
```

In `actor.rs` handler:

```rust
SagaEvent { event } => handle_saga_event(self, event),
RecoverSagas { reply_to } => { let _ = reply_tell(reply_to, recover_sagas(self)); }
GetSagaStats { reply_to } => { let _ = reply_tell(reply_to, self.saga_stats.snapshot()); }
```

PubSub conventions:

- Topic name: `SagaChoreographyEvent::topic(saga_type)` (`saga:<type>`).
- Subscribe each participant mailbox to every saga type it returns from `saga_types()`.
- Start a saga by publishing `SagaChoreographyEvent::SagaStarted { context, payload }`.
- Publish follow-up choreography events (`StepCompleted`, `StepFailed`, `CompensationRequested`, etc.) through pubsub.

## Observability Pattern

Use both stats and tracing.

- Keep `Arc<ParticipantStats>` in actor state and expose `GetSagaStats`.
- Emit structured `tracing` logs in participant hooks (`on_saga_completed`, `on_saga_failed`, `on_quarantined`) and/or around pubsub boundaries.
- Include at least: `saga_id`, `saga_type`, `step_name`, `trace_id`, and failure reason.
- If you need a pluggable telemetry sink, wire a `SagaObserver` implementation (e.g. `TracingObserver`) in your actor runtime path.

## Recovery + Idempotency Rules

- Call `recover_sagas(self)` during startup/recovery commands to find non-terminal sagas.
- Do not bypass dedupe/journal paths; `handle_saga_event` already enforces dedupe via `trace_id:event_type`.
- Prune saga state/dedupe keys on terminal events where appropriate (`SagaCompleted`, `SagaFailed`).

## Minimal Implementation Checklist

1. Create actor folder layout (including `saga/participant.rs` and `saga/state_ext.rs`).
2. Add required actor fields (`saga_states`, `saga_journal`, `saga_dedupe`, `saga_stats`).
3. Implement `SagaStateExt` (all methods) and `SagaParticipant` (required methods).
4. Add `SagaEvent`/`RecoverSagas`/`GetSagaStats` commands.
5. Route `SagaEvent` to `handle_saga_event(self, event)`.
6. Subscribe mailbox to `saga:<type>` topic(s), publish `SagaStarted`, and emit choreography events.
7. Add tracing + stats exposure for ops/observability.
