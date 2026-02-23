# Choreography-Based SAGA Architecture Plan

## Executive Summary

A choreography-based SAGA pattern that integrates **natively** with `icanact-core` actors.

**Key Principle**: Actors ARE the microservices. Saga choreography is just another actor behavior using tell/ask/pubsub.

---

## Clear Separation: Framework vs Actor Implementation

### Framework Level (`icanact-saga-choreography` crate)

Everything in this crate is **reusable** and **common** to all saga participants:

```
icanact-saga-choreography/
├── src/
│   ├── lib.rs                    # Re-exports
│   │
│   ├── types/                    # CORE TYPES (framework provides)
│   │   ├── mod.rs
│   │   ├── context.rs            # SagaId, SagaContext, PeerId
│   │   ├── idempotency.rs        # IdempotencyKey
│   │   └── timestamp.rs          # Timestamp utilities
│   │
│   ├── state/                    # TYPESTATE STATES (framework provides)
│   │   ├── mod.rs
│   │   ├── markers.rs            # StepState, TerminalState traits
│   │   ├── states.rs             # Idle, Triggered, Executing, Completed, Failed, Compensating, Compensated, Quarantined
│   │   ├── container.rs          # SagaParticipantState<S>, SagaStateEntry
│   │   └── transitions.rs        # State transition impls
│   │
│   ├── events/                   # EVENTS (framework provides)
│   │   ├── mod.rs
│   │   ├── choreography.rs       # SagaChoreographyEvent
│   │   └── participant.rs        # ParticipantEvent
│   │
│   ├── errors.rs                 # StepOutput, StepError, CompensationError
│   │
│   ├── traits/                   # TRAITS (actor implements)
│   │   ├── mod.rs
│   │   ├── participant.rs        # SagaParticipant trait
│   │   ├── state_ext.rs          # SagaStateExt trait
│   │   └── subscription.rs       # DependencySpec, RetryPolicy
│   │
│   ├── storage/                  # STORAGE TRAITS (framework provides, actor supplies impl)
│   │   ├── mod.rs
│   │   ├── journal.rs            # ParticipantJournal trait
│   │   ├── dedupe.rs             # ParticipantDedupeStore trait
│   │   └── heed_impl.rs          # Heed (LMDB) implementation
│   │
│   ├── observability/            # OBSERVABILITY (framework provides)
│   │   ├── mod.rs
│   │   ├── stats.rs              # ParticipantStats
│   │   └── observer.rs           # SagaObserver trait
│   │
│   └── helpers/                  # HELPERS (framework provides)
        ├── mod.rs
        ├── handler.rs            # handle_saga_event()
        ├── execution.rs          # execute_step_wrapper(), compensate_wrapper()
        └── recovery.rs           # recover_sagas()
```

### Actor Level (what YOU implement for each saga participant)

Each actor that participates in sagas follows this **consistent structure**:

```
src/actors/my-actor/
├── mod.rs                        # Re-exports
├── actor.rs                      # Core actor (impl Actor)
├── business.rs                   # Business logic (pure functions, no actor deps)
├── messaging.rs                  # Message types (Command enum)
│
├── saga/                         # SAGA IMPLEMENTATION (actor-specific)
│   ├── mod.rs                    # Re-exports saga impl
│   ├── participant.rs            # impl SagaParticipant for MyActor
│   ├── state_ext.rs              # impl SagaStateExt for MyActor
│   ├── handler.rs                # Saga event handler (calls helpers)
│   └── types.rs                  # Actor-specific saga types (optional)
│
├── tests/
│   ├── unit.rs                   # Unit tests
│   ├── integration.rs            # Integration with wire mocks
│   └── saga.rs                   # Saga-specific tests
│
└── architecture.md               # Actor purpose, diagrams, saga role
```

---

## What the Actor Implements

### Required: Two Traits

Every saga participant implements exactly **two traits**:

#### 1. `SagaStateExt` - Infrastructure Access

```rust
// File: src/actors/my-actor/saga/state_ext.rs

use icanact_saga_choreography::{SagaStateExt, SagaId, SagaStateEntry, 
    ParticipantJournal, ParticipantDedupeStore};
use std::collections::HashMap;
use std::sync::Arc;

impl SagaStateExt for MyActor {
    /// Provide access to saga states map
    fn saga_states(&mut self) -> &mut HashMap<SagaId, SagaStateEntry> {
        &mut self.saga_states
    }
    
    /// Provide access to journal
    fn saga_journal(&self) -> &Arc<dyn ParticipantJournal> {
        &self.saga_journal
    }
    
    /// Provide access to dedupe store
    fn saga_dedupe(&self) -> &Arc<dyn ParticipantDedupeStore> {
        &self.saga_dedupe
    }
    
    /// Provide time source
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}
```

#### 2. `SagaParticipant` - Business Behavior

```rust
// File: src/actors/my-actor/saga/participant.rs

use icanact_saga_choreography::{
    SagaParticipant, SagaContext, DependencySpec, RetryPolicy,
    StepOutput, StepError, CompensationError,
};

impl SagaParticipant for MyActor {
    type Error = MyError;
    
    // === REQUIRED: Identity ===
    
    fn step_name(&self) -> &str {
        "my_step"  // This actor's step name in the saga
    }
    
    fn saga_types(&self) -> &[&'static str] {
        &["my_workflow"]  // Which saga types this actor joins
    }
    
    // === REQUIRED: Forward Step ===
    
    fn execute_step(
        &mut self,
        context: &SagaContext,
        input: &[u8],
    ) -> Result<StepOutput, StepError> {
        // 1. Deserialize input
        let my_input: MyStepInput = bincode::deserialize(input)
            .map_err(|e| StepError::Terminal { 
                reason: format!("deserialize: {}", e).into() 
            })?;
        
        // 2. Call business logic (pure function, testable)
        let result = business::execute_my_step(&my_input, &self.dependencies)
            .map_err(|e| StepError::RequireCompensation { 
                reason: e.to_string().into() 
            })?;
        
        // 3. Return output + compensation data
        Ok(StepOutput::Completed {
            output: bincode::serialize(&result.output).unwrap_or_default(),
            compensation_data: bincode::serialize(&result.undo_info).unwrap_or_default(),
        })
    }
    
    // === REQUIRED: Compensation ===
    
    fn compensate_step(
        &mut self,
        context: &SagaContext,
        compensation_data: &[u8],
    ) -> Result<(), CompensationError> {
        let undo_info: UndoInfo = bincode::deserialize(compensation_data)
            .map_err(|e| CompensationError::Terminal { 
                reason: format!("deserialize: {}", e).into() 
            })?;
        
        business::compensate_my_step(&undo_info, &self.dependencies)
            .map_err(|e| CompensationError::Ambiguous { 
                reason: e.to_string().into() 
            })
    }
    
    // === OPTIONAL: Configuration ===
    
    fn depends_on(&self) -> DependencySpec {
        DependencySpec::After("previous_step")  // When to execute
    }
    
    fn retry_policy(&self) -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            initial_delay_millis: 1000,
            max_delay_millis: 10000,
            backoff_multiplier: 2.0,
        }
    }
    
    // === OPTIONAL: Hooks ===
    
    fn on_saga_completed(&mut self, context: &SagaContext) {
        tracing::info!(saga_id = %context.saga_id, "Saga completed");
    }
    
    fn on_quarantined(&mut self, context: &SagaContext, reason: &str) {
        tracing::error!(saga_id = %context.saga_id, reason = %reason, "Quarantined");
    }
}
```

### Required: Actor State Fields

The actor struct must include these fields:

```rust
// File: src/actors/my-actor/actor.rs

use icanact_saga_choreography::{
    SagaId, SagaStateEntry, ParticipantJournal, 
    ParticipantDedupeStore, ParticipantStats,
};
use std::collections::HashMap;
use std::sync::Arc;

pub struct MyActor {
    // === Business State ===
    pub orders: HashMap<OrderId, Order>,
    
    // === Actor Dependencies ===
    pub ws_actor: MailboxAddr<WSCommand>,
    pub db_actor: MailboxAddr<DbCommand>,
    
    // === SAGA STATE (required for SagaStateExt) ===
    pub saga_states: HashMap<SagaId, SagaStateEntry>,
    pub saga_journal: Arc<dyn ParticipantJournal>,
    pub saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    pub saga_stats: Arc<ParticipantStats>,
}
```

### Required: Message Variant

The actor's command enum must include a saga event variant:

```rust
// File: src/actors/my-actor/messaging.rs

use icanact_saga_choreography::SagaChoreographyEvent;

pub enum MyActorCommand {
    // Business commands
    DoSomething { /* ... */ },
    
    // Saga event (from pubsub subscription)
    SagaEvent { event: SagaChoreographyEvent },
    
    // Admin
    RecoverSagas { reply_to: ReplyTo<Vec<SagaId>> },
    GetSagaStats { reply_to: ReplyTo<ParticipantStatsSnapshot> },
}
```

### Required: Handler Integration

Wire saga events to the handler in Actor::handle:

```rust
// File: src/actors/my-actor/actor.rs

use icanact_core::Actor;
use icanact_saga_choreography::handle_saga_event;

impl Actor for MyActor {
    type Msg = MyActorCommand;
    
    fn handle(&mut self, msg: Self::Msg) {
        match msg {
            MyActorCommand::DoSomething { .. } => {
                // Business logic
            }
            
            // Wire saga events to framework handler
            MyActorCommand::SagaEvent { event } => {
                handle_saga_event(self, event);
            }
            
            MyActorCommand::RecoverSagas { reply_to } => {
                let ids = icanact_saga_choreography::recover_sagas(self);
                let _ = reply_tell(reply_to, ids);
            }
            
            MyActorCommand::GetSagaStats { reply_to } => {
                let _ = reply_tell(reply_to, self.saga_stats.snapshot());
            }
        }
    }
}
```

---

## Summary: Actor Checklist

For each saga participant, create these files:

| File | Purpose | Framework or Actor? |
|------|---------|---------------------|
| `saga/mod.rs` | Re-exports | **Actor** |
| `saga/participant.rs` | `impl SagaParticipant` | **Actor** (business logic) |
| `saga/state_ext.rs` | `impl SagaStateExt` | **Actor** (boilerplate) |
| `saga/handler.rs` | Optional custom handling | **Actor** (rarely needed) |
| `saga/types.rs` | Actor-specific types | **Actor** (if needed) |
| Actor struct fields | saga_states, journal, dedupe, stats | **Actor** |
| Command enum | `SagaEvent { event }` variant | **Actor** |
| Actor::handle | Wire to `handle_saga_event()` | **Actor** |

---

## Framework Exports (what you USE)

```rust
// From icanact-saga-choreography, you use:

// Types
pub use crate::types::{SagaId, SagaContext, PeerId, IdempotencyKey};

// State (typestate)
pub use crate::state::{
    SagaParticipantState, SagaStateEntry, TimestampedEvent,
    Idle, Triggered, Executing, Completed, Failed,
    Compensating, Compensated, Quarantined,
};

// Events
pub use crate::events::{SagaChoreographyEvent, ParticipantEvent};

// Errors
pub use crate::errors::{StepOutput, StepError, CompensationError};

// Traits (implement these)
pub use crate::traits::{SagaParticipant, SagaStateExt, DependencySpec, RetryPolicy};

// Storage
pub use crate::storage::{ParticipantJournal, ParticipantDedupeStore, HeedJournal, HeedDedupe};

// Observability
pub use crate::observability::{ParticipantStats, ParticipantStatsSnapshot, SagaObserver};

// Helpers
pub use crate::helpers::{handle_saga_event, execute_step_wrapper, compensate_wrapper, recover_sagas};
```

---

## Storage: Heed (LMDB)

We use **heed** (LMDB wrapper) for persistent storage - same as the existing icanact-saga.

### Why Heed/LMDB

- **Very fast** - Memory-mapped, minimal overhead
- **Per-actor** - Each actor can have its own database/env
- **ACID** - Durable writes, crash recovery
- **Battle-tested** - LMDB is extremely stable
- **Already integrated** - Consistent with icanact-saga

### Storage Trait Abstraction

```rust
pub trait ParticipantJournal: Send + Sync + 'static {
    fn append(&self, saga_id: SagaId, event: ParticipantEvent) -> Result<u64, JournalError>;
    fn read(&self, saga_id: SagaId) -> Result<Vec<JournalEntry>, JournalError>;
    fn list_sagas(&self) -> Result<Vec<SagaId>, JournalError>;
}

pub trait ParticipantDedupeStore: Send + Sync + 'static {
    fn check_and_mark(&self, saga_id: SagaId, key: &str) -> Result<bool, DedupeError>;
    fn prune(&self, saga_id: SagaId) -> Result<(), DedupeError>;
}
```

### Heed Implementation

```rust
// storage/heed_impl.rs

use heed::{Env, Database, types::*};
use std::sync::Arc;

pub struct HeedJournal {
    env: Arc<Env>,
    db: Database<OwnedType<u64>, ByteSlice>,  // saga_id -> events
}

pub struct HeedDedupe {
    env: Arc<Env>,
    db: Database<ByteSlice, Unit>,  // (saga_id, key) -> ()
}
```

---

## Deribit Order Example Structure

```
examples/deribit/
├── main.rs                          # Bootstrap all actors
├── saga/
│   ├── mod.rs                       # Deribit saga types
│   └── types.rs                     # DeribitOrderPayload, etc.
│
├── actors/
│   ├── risk_manager/                # SAGA INITIATOR
│   │   ├── mod.rs
│   │   ├── actor.rs                 # impl Actor, starts sagas
│   │   ├── messaging.rs
│   │   ├── business.rs
│   │   └── saga/
│   │       ├── mod.rs
│   │       ├── participant.rs       # impl SagaParticipant
│   │       └── state_ext.rs         # impl SagaStateExt
│   │
│   ├── order_placer/                # STEP: prepare_order
│   │   ├── mod.rs
│   │   ├── actor.rs
│   │   ├── messaging.rs
│   │   ├── business.rs
│   │   └── saga/
│   │       ├── mod.rs
│   │       ├── participant.rs       # depends_on: OnSagaStart
│   │       └── state_ext.rs
│   │
│   ├── order_coordinator/           # STEP: place_order
│   │   ├── mod.rs
│   │   ├── actor.rs
│   │   ├── messaging.rs
│   │   ├── business.rs
│   │   └── saga/
│   │       ├── mod.rs
│   │       ├── participant.rs       # depends_on: After("prepare_order")
│   │       └── state_ext.rs
│   │
│   ├── deribit_ws/                  # NOT a saga participant (just WS handler)
│   │   ├── mod.rs
│   │   ├── actor.rs                 # async actor, owns WS connection
│   │   ├── messaging.rs
│   │   └── business.rs
│   │
│   └── order_monitor/               # Optional: monitors order state
│       ├── mod.rs
│       ├── actor.rs
│       ├── messaging.rs
│       └── saga/
│           ├── mod.rs
│           ├── participant.rs       # depends_on: After("place_order")
│           └── state_ext.rs
```

---

## Actor Structure Template

Copy this for each new saga participant:

```
src/actors/{actor-name}/
├── mod.rs
├── actor.rs              # struct MyActor, impl Actor
├── messaging.rs          # pub enum MyActorCommand
├── business.rs           # pub fn execute_step(...), pub fn compensate(...)
│
├── saga/
│   ├── mod.rs            # pub mod participant; pub mod state_ext;
│   ├── participant.rs    # impl SagaParticipant for MyActor
│   ├── state_ext.rs      # impl SagaStateExt for MyActor
│   └── types.rs          # (optional) actor-specific types
│
├── tests/
│   ├── mod.rs
│   ├── unit.rs
│   └── saga.rs
│
└── architecture.md
```

### File Templates

#### `mod.rs`
```rust
mod actor;
mod messaging;
mod business;
mod saga;

pub use actor::MyActor;
pub use messaging::MyActorCommand;
```

#### `actor.rs`
```rust
use icanact_core::local_sync::{Actor, MailboxAddr, ReplyTo};
use icanact_saga_choreography::{
    SagaId, SagaStateEntry, ParticipantJournal, 
    ParticipantDedupeStore, ParticipantStats, handle_saga_event,
};
use std::collections::HashMap;
use std::sync::Arc;

pub struct MyActor {
    // Business state
    // ...
    
    // Dependencies
    // ...
    
    // Saga state (required)
    saga_states: HashMap<SagaId, SagaStateEntry>,
    saga_journal: Arc<dyn ParticipantJournal>,
    saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    saga_stats: Arc<ParticipantStats>,
}

impl MyActor {
    pub fn new(
        saga_journal: Arc<dyn ParticipantJournal>,
        saga_dedupe: Arc<dyn ParticipantDedupeStore>,
    ) -> Self {
        Self {
            saga_states: HashMap::new(),
            saga_journal,
            saga_dedupe,
            saga_stats: Arc::new(ParticipantStats::new()),
        }
    }
}

impl Actor for MyActor {
    type Msg = super::messaging::MyActorCommand;
    
    fn handle(&mut self, msg: Self::Msg) {
        use super::messaging::MyActorCommand::*;
        
        match msg {
            SagaEvent { event } => handle_saga_event(self, event),
            RecoverSagas { reply_to } => { /* ... */ },
            GetSagaStats { reply_to } => { /* ... */ },
            // Business messages...
            _ => {}
        }
    }
}
```

#### `messaging.rs`
```rust
use icanact_core::local_sync::ReplyTo;
use icanact_saga_choreography::{SagaChoreographyEvent, SagaId, ParticipantStatsSnapshot};

pub enum MyActorCommand {
    // Business commands
    DoSomething { reply_to: ReplyTo<Result<(), Box<str>>> },
    
    // Saga (required)
    SagaEvent { event: SagaChoreographyEvent },
    RecoverSagas { reply_to: ReplyTo<Vec<SagaId>> },
    GetSagaStats { reply_to: ReplyTo<ParticipantStatsSnapshot> },
}
```

#### `saga/mod.rs`
```rust
pub mod participant;
pub mod state_ext;
// pub mod types;  // if needed
```

#### `saga/participant.rs`
```rust
use crate::{MyActor, MyError};
use icanact_saga_choreography::{
    SagaParticipant, SagaContext, DependencySpec, RetryPolicy,
    StepOutput, StepError, CompensationError,
};

impl SagaParticipant for MyActor {
    type Error = MyError;
    
    fn step_name(&self) -> &str { "my_step" }
    fn saga_types(&self) -> &[&'static str] { &["my_workflow"] }
    
    fn execute_step(&mut self, ctx: &SagaContext, input: &[u8]) 
        -> Result<StepOutput, StepError> 
    {
        // Call business.rs
        todo!()
    }
    
    fn compensate_step(&mut self, ctx: &SagaContext, data: &[u8]) 
        -> Result<(), CompensationError> 
    {
        // Call business.rs
        todo!()
    }
    
    fn depends_on(&self) -> DependencySpec {
        DependencySpec::OnSagaStart  // or After("other_step")
    }
}
```

#### `saga/state_ext.rs`
```rust
use crate::MyActor;
use icanact_saga_choreography::{SagaStateExt, SagaId, SagaStateEntry,
    ParticipantJournal, ParticipantDedupeStore};
use std::collections::HashMap;
use std::sync::Arc;

impl SagaStateExt for MyActor {
    fn saga_states(&mut self) -> &mut HashMap<SagaId, SagaStateEntry> {
        &mut self.saga_states
    }
    
    fn saga_journal(&self) -> &Arc<dyn ParticipantJournal> {
        &self.saga_journal
    }
    
    fn saga_dedupe(&self) -> &Arc<dyn ParticipantDedupeStore> {
        &self.saga_dedupe
    }
    
    fn now_millis(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}
```

#### `business.rs`
```rust
/// Pure business logic - no actor dependencies, easily testable
pub fn execute_step(input: &StepInput, deps: &Dependencies) 
    -> Result<StepResult, BusinessError> 
{
    // Pure logic here
    todo!()
}

pub fn compensate(undo_info: &UndoInfo, deps: &Dependencies) 
    -> Result<(), CompensationError> 
{
    // Pure logic here
    todo!()
}
```

---

## Key Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Framework vs Actor** | Framework provides types/traits/helpers, actor implements 2 traits | Clear separation, minimal per-actor code |
| **State Storage** | In actor struct (HashMap<SagaId, SagaStateEntry>) | Local, fast, actor owns its state |
| **Persistent Storage** | Heed (LMDB) | Fast, per-actor, already used in icanact-saga |
| **Event Handling** | `handle_saga_event()` helper | Single function call in Actor::handle |
| **Business Logic** | Separate file, pure functions | Testable, no actor deps |

---

## Deribit Order Example in `icanact-examples`

The full Deribit order workflow example should be created in the `icanact-examples` repository:

```
icanact-examples/
├── Cargo.toml                          # Add icanact-saga-choreography dependency
│
└── examples/
    └── deribit-order-saga/
        ├── Cargo.toml
        │
        ├── src/
        │   ├── main.rs                 # Bootstrap
        │   │
        │   ├── saga/
        │   │   ├── mod.rs
        │   │   └── types.rs            # DeribitOrderPayload, etc.
        │   │
        │   └── actors/
        │       ├── mod.rs
        │       │
        │       ├── ta_signal/           # Signal generator
        │       │   ├── mod.rs
        │       │   ├── actor.rs
        │       │   ├── messaging.rs
        │       │   ├── business.rs
        │       │   └── tests/
        │       │       └── mod.rs
        │       │
        │       ├── risk_manager/        # SAGA INITIATOR
        │       │   ├── mod.rs
        │       │   ├── actor.rs
        │       │   ├── messaging.rs
        │       │   ├── business.rs
        │       │   ├── saga/
        │       │   │   ├── mod.rs
        │       │   │   ├── participant.rs
        │       │   │   └── state_ext.rs
        │       │   └── tests/
        │       │       └── saga.rs
        │       │
        │       ├── order_placer/        # STEP: prepare_order
        │       │   ├── mod.rs
        │       │   ├── actor.rs
        │       │   ├── messaging.rs
        │       │   ├── business.rs
        │       │   ├── saga/
        │       │   │   ├── mod.rs
        │       │   │   ├── participant.rs
        │       │   │   └── state_ext.rs
        │       │   └── tests/
        │       │
        │       ├── order_coordinator/   # STEP: place_order
        │       │   ├── mod.rs
        │       │   ├── actor.rs
        │       │   ├── messaging.rs
        │       │   ├── business.rs
        │       │   ├── saga/
        │       │   │   ├── mod.rs
        │       │   │   ├── participant.rs
        │       │   │   └── state_ext.rs
        │       │   └── tests/
        │       │
        │       ├── deribit_ws/          # NOT a saga participant
        │       │   ├── mod.rs
        │       │   ├── actor.rs         # async actor
        │       │   ├── messaging.rs
        │       │   └── business.rs
        │       │
        │       ├── order_monitor/       # STEP: monitor_order
        │       │   ├── mod.rs
        │       │   ├── actor.rs
        │       │   ├── messaging.rs
        │       │   ├── business.rs
        │       │   ├── saga/
        │       │   │   ├── mod.rs
        │       │   │   ├── participant.rs
        │       │   │   └── state_ext.rs
        │       │   └── tests/
        │       │
        │       └── rate_limiter/        # NOT a saga participant
        │           ├── mod.rs
        │           ├── actor.rs
        │           ├── messaging.rs
        │           └── tests/
        │
        └── architecture.md              # Overview, flow diagrams
```

### Example Cargo.toml

```toml
[package]
name = "deribit-order-saga"
version = "0.1.0"
edition = "2021"

[dependencies]
icanact-core = { path = "../../icanact-core" }
icanact-saga-choreography = { path = "../../icanact-saga-choreography" }
tokio = { version = "1", features = ["rt", "rt-multi-thread", "time", "macros"] }
tracing = "0.1"
tracing-subscriber = "0.3"
serde = { version = "1", features = ["derive"] }
bincode = "1.3"
uuid = { version = "1", features = ["v4"] }

[dev-dependencies]
tempfile = "3"
```

### Flow Diagram (for architecture.md)

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        DERIBIT ORDER SAGA FLOW                               │
│                                                                             │
│  Market Data                                                                │
│      │                                                                      │
│      ▼                                                                      │
│  ┌──────────────┐                                                          │
│  │ TASignal     │ SignalEvent                                              │
│  │ Actor        │──────────────────┐                                       │
│  │ (sync)       │                  │                                       │
│  └──────────────┘                  ▼                                       │
│                           ┌──────────────┐                                  │
│                           │RiskManager   │                                  │
│                           │ Actor        │                                  │
│                           │ (sync)       │                                  │
│                           │              │                                  │
│                           │ GO ──────────┼──▶ SagaStarted ──────────────┐  │
│                           │ NOGO         │   (pubsub)                   │  │
│                           └──────────────┘                               │  │
│                                   ▲                                       │  │
│                                   │ check                                 │  │
│                           ┌──────┴───────┐                                │  │
│                           │RateLimiter   │                                │  │
│                           │ Actor        │                                │  │
│                           └──────────────┘                                │  │
│                                                                          │  │
│  ┌─────────────────────────────────────────────────────────────────────┐  │  │
│  │                      SAGA: deribit_order                             │  │  │
│  │                                                                      │  │  │
│  │  SagaStarted ─────────────────────────────────────────────────────▶ │  │  │
│  │       │                                                              │  │  │
│  │       ▼                                                              │  │  │
│  │  ┌──────────────┐                                                    │  │  │
│  │  │OrderPlacer   │ prepare_order                                      │  │  │
│  │  │ Actor        │─────────────────────┐                              │  │  │
│  │  │ (sync)       │                     │                              │  │  │
│  │  │ saga/        │                     ▼                              │  │  │
│  │  └──────────────┘              StepCompleted ──────────────────┐    │  │  │
│  │       depends_on:               (prepare_order)                │    │  │  │
│  │       OnSagaStart                                              │    │  │  │
│  │                                                                ▼    │  │  │
│  │  ┌──────────────┐                                         ┌────┴───┐│  │  │
│  │  │OrderCoord    │ place_order                             │Deribit ││  │  │
│  │  │ Actor        │◀────────────────────────────────────────│WS      ││  │  │
│  │  │ (sync)       │              ask                         │Actor   ││  │  │
│  │  │ saga/        │                                         │(async) ││  │  │
│  │  └──────────────┘                                         └────────┘│  │  │
│  │       depends_on:                                                   │  │  │
│  │       After("prepare_order")                                        │  │  │
│  │       │                                                             │  │  │
│  │       ▼                                                             │  │  │
│  │  StepCompleted (place_order) ───────────────────────────────────┐  │  │  │
│  │       │                                                          │  │  │  │
│  │       ▼                                                          │  │  │  │
│  │  ┌──────────────┐                                                │  │  │  │
│  │  │OrderMonitor  │ monitor_order                                  │  │  │  │
│  │  │ Actor        │◀──────────────── order updates ────────────────┘  │  │  │
│  │  │ (sync)       │                                                   │  │  │
│  │  │ saga/        │                                                   │  │  │
│  │  └──────────────┘                                                   │  │  │
│  │       │                                                             │  │  │
│  │       ▼ (order filled)                                              │  │  │
│  │  SagaCompleted ◀────────────────────────────────────────────────────┘  │  │
│  │                                                                       │  │
│  │  ON FAILURE:                                                          │  │
│  │       │                                                               │  │  │
│  │       ▼                                                               │  │  │
│  │  CompensationRequested ───────────────────────────────────────────────┼──┘
│  │       │                                                               │
│  │       ▼                                                               │
│  │  Each participant with Completed state:                               │
│  │       compensate_step()                                               │
│  │       emit CompensationCompleted                                      │
│  │                                                                       │
│  └───────────────────────────────────────────────────────────────────────┘
│                                                                          │
└─────────────────────────────────────────────────────────────────────────────┘
```

### Test Infrastructure

The example includes comprehensive test infrastructure:

```
deribit-order-saga/
├── tests/
│   ├── common/
│   │   └── mod.rs           # Test harness, SagaEventCollector, MockWSBehavior
│   ├── mock_ws.rs           # Mock Deribit WS actor with connection kill
│   └── e2e_saga_tests.rs    # 18+ e2e tests covering edge cases
```

#### Mock WebSocket Actor

The `MockDeribitWSActor` provides controlled responses for testing:

```rust
let ws_behavior = MockWSBehavior {
    place_order_response: Some(Ok(DeribitWSResponse::OrderPlaced {
        order_id: "order-test-123".into(),
    })),
    cancel_order_response: Some(Ok(DeribitWSResponse::OrderCancelled)),
    should_drop_connection: false,
    delay_millis: 0,
};

let harness = TestHarness::with_behavior(ws_behavior);
```

#### Connection Kill Testing

Simulate connection failures during saga execution:

```rust
// Kill connection during order placement
let ws_behavior = MockWSBehavior {
    should_drop_connection: true,
    ..Default::default()
};
let harness = TestHarness::with_behavior(ws_behavior);

// Or kill connection after order placed (during compensation)
harness.drop_ws_connection();
```

#### E2E Test Coverage

| Test | Description |
|------|-------------|
| `test_happy_path_order_placed_and_filled` | Complete successful order flow |
| `test_risk_rejected_low_confidence` | Signal rejected, no saga started |
| `test_instrument_blocked_during_active_saga` | Concurrent orders blocked |
| `test_order_rejected_triggers_compensation` | Order reject → compensation |
| `test_connection_kill_during_order_placement` | WS drop during ask |
| `test_connection_kill_during_compensation` | WS drop during cancel |
| `test_ws_error_response` | Error response handling |
| `test_multiple_different_instruments` | Parallel sagas |
| `test_sell_signal_flow` | Sell order flow |
| `test_duplicate_event_handling` | Idempotency verification |
| `test_order_filled_completes_saga` | Monitor → SagaCompleted |
| `test_order_cancelled_fails_saga` | Monitor → SagaFailed |
| `test_exposure_limit_exceeded` | Risk limit enforcement |
| `test_concurrent_sagas_different_instruments` | Parallel execution |
| `test_saga_quarantine_on_compensation_failure` | Quarantine flow |
| `test_circuit_breaker_on_repeated_failures` | Repeated failures |

