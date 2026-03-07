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
│   └── types.rs            # optional
├── tests/
│   ├── unit.rs
│   ├── integration.rs      # optional
│   └── saga.rs
└── architecture.md         # short role/flow notes
```

## Required Actor Fields

The canonical integration is one embedded saga support field:

```rust
pub struct MyActor {
    // business state + dependencies...
    saga: SagaParticipantSupport<MyJournal, MyDedupe>,
}
```

`SagaParticipantSupport` owns:

- saga state storage
- journal
- dedupe store
- participant stats
- startup recovery events
- optional local pubsub bus

## Required Traits

Implement exactly these two traits for each saga participant.

### 1) `HasSagaParticipantSupport` (canonical infrastructure access)

Preferred integration:

```rust
impl HasSagaParticipantSupport for MyActor {
    type Journal = MyJournal;
    type Dedupe = MyDedupe;

    fn saga_support(&self) -> &SagaParticipantSupport<Self::Journal, Self::Dedupe> {
        &self.saga
    }

    fn saga_support_mut(&mut self) -> &mut SagaParticipantSupport<Self::Journal, Self::Dedupe> {
        &mut self.saga
    }
}
```

After that, `SagaStateExt` is provided automatically by the crate.

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
GetSagaStats { reply_to } => { let _ = reply_tell(reply_to, self.saga.stats.snapshot()); }
```

PubSub conventions:

- Topic name: `SagaChoreographyEvent::topic(saga_type)` (`saga:<type>`).
- Subscribe each participant mailbox to every saga type it returns from `saga_types()`.
- Start a saga by publishing `SagaChoreographyEvent::SagaStarted { context, payload }`.
- Publish follow-up choreography events (`StepCompleted`, `StepFailed`, `CompensationRequested`, etc.) through pubsub.

## Observability Pattern

Use both stats and tracing.

- Keep participant stats in `self.saga.stats` and expose `GetSagaStats`.
- Emit structured `tracing` logs in participant hooks (`on_saga_completed`, `on_saga_failed`, `on_quarantined`) and/or around pubsub boundaries.
- Include at least: `saga_id`, `saga_type`, `step_name`, `trace_id`, and failure reason.
- If you need a pluggable telemetry sink, wire a `SagaObserver` implementation (e.g. `TracingObserver`) in your actor runtime path.

## Recovery + Idempotency Rules

- Call `recover_sagas(self)` during startup/recovery commands to find non-terminal sagas.
- Do not bypass dedupe/journal paths; `handle_saga_event` already enforces dedupe via `trace_id:event_type`.
- Prune saga state/dedupe keys on terminal events where appropriate (`SagaCompleted`, `SagaFailed`).

## Minimal Implementation Checklist

1. Create actor folder layout (including `saga/participant.rs`).
2. Add `saga: SagaParticipantSupport<Journal, Dedupe>`.
3. Implement `HasSagaParticipantSupport` and `SagaParticipant`.
4. Add `SagaEvent`/`RecoverSagas`/`GetSagaStats` commands.
5. Route `SagaEvent` to `handle_saga_event(self, event)`.
6. Subscribe mailbox to `saga:<type>` topic(s), publish `SagaStarted`, and emit choreography events.
7. Add tracing + stats exposure for ops/observability.
