//! Saga observer trait

use super::SagaContext;

/// Observer trait for external observability.
///
/// Implement this trait to receive callbacks for saga lifecycle events.
/// Observers must be `Send + Sync + 'static` to support concurrent access
/// from multiple saga participants.
///
/// # Example
///
/// ```ignore
/// struct MyObserver;
///
/// impl SagaObserver for MyObserver {
///     fn on_saga_started(&self, context: &SagaContext) {
///         println!("Saga {} started", context.saga_id.0);
///     }
///     // ... implement other methods
/// }
/// ```
pub trait SagaObserver: Send + Sync + 'static {
    /// Called when a saga instance is started.
    ///
    /// @param context - The saga context containing saga_id, saga_type, and other metadata
    fn on_saga_started(&self, context: &SagaContext);

    /// Called when a step begins execution.
    ///
    /// @param context - The saga context
    /// @param step - The name/identifier of the step being executed
    fn on_step_started(&self, context: &SagaContext, step: &str);

    /// Called when a step completes successfully.
    ///
    /// @param context - The saga context
    /// @param step - The name/identifier of the completed step
    /// @param duration_millis - The execution time of the step in milliseconds
    fn on_step_completed(&self, context: &SagaContext, step: &str, duration_millis: u64);

    /// Called when a step fails during execution.
    ///
    /// @param context - The saga context
    /// @param step - The name/identifier of the failed step
    /// @param error - A description of the error that occurred
    fn on_step_failed(&self, context: &SagaContext, step: &str, error: &str);

    /// Called when compensation begins for a previously completed step.
    ///
    /// @param context - The saga context
    /// @param step - The name/identifier of the step being compensated
    fn on_compensation_started(&self, context: &SagaContext, step: &str);

    /// Called when compensation completes for a step.
    ///
    /// @param context - The saga context
    /// @param step - The name/identifier of the step whose compensation completed
    fn on_compensation_completed(&self, context: &SagaContext, step: &str);

    /// Called when a saga completes successfully (all steps finished).
    ///
    /// @param context - The saga context
    fn on_saga_completed(&self, context: &SagaContext);

    /// Called when a saga fails and cannot continue.
    ///
    /// @param context - The saga context
    /// @param reason - A description of why the saga failed
    fn on_saga_failed(&self, context: &SagaContext, reason: &str);

    /// Called when a saga is quarantined due to unrecoverable errors.
    ///
    /// Quarantined sagas require manual intervention to resolve.
    ///
    /// @param context - The saga context
    /// @param step - The name/identifier of the step that caused the quarantine
    /// @param reason - A description of why the saga was quarantined
    fn on_saga_quarantined(&self, context: &SagaContext, step: &str, reason: &str);
}

/// A no-operation observer that ignores all saga events.
///
/// Use this when you need an observer but don't want any observability overhead.
/// All callback methods are empty implementations.
pub struct NoOpObserver;

impl SagaObserver for NoOpObserver {
    fn on_saga_started(&self, _context: &SagaContext) {}
    fn on_step_started(&self, _context: &SagaContext, _step: &str) {}
    fn on_step_completed(&self, _context: &SagaContext, _step: &str, _duration_millis: u64) {}
    fn on_step_failed(&self, _context: &SagaContext, _step: &str, _error: &str) {}
    fn on_compensation_started(&self, _context: &SagaContext, _step: &str) {}
    fn on_compensation_completed(&self, _context: &SagaContext, _step: &str) {}
    fn on_saga_completed(&self, _context: &SagaContext) {}
    fn on_saga_failed(&self, _context: &SagaContext, _reason: &str) {}
    fn on_saga_quarantined(&self, _context: &SagaContext, _step: &str, _reason: &str) {}
}

/// An observer that emits structured log events using the `tracing` crate.
///
/// This observer logs all saga lifecycle events at appropriate log levels:
/// - `INFO`: Normal operations (saga started, step started/completed, compensation events)
/// - `WARN`: Step failures
/// - `ERROR`: Saga failures and quarantines
///
/// Each log event includes structured fields for `saga_id`, and where applicable,
/// `step`, `duration_ms`, `error`, or `reason`.
pub struct TracingObserver;

impl SagaObserver for TracingObserver {
    fn on_saga_started(&self, context: &SagaContext) {
        tracing::info!(saga_id = %context.saga_id.0, saga_type = %context.saga_type, "Saga started");
    }

    fn on_step_started(&self, context: &SagaContext, step: &str) {
        tracing::info!(saga_id = %context.saga_id.0, step = %step, "Step started");
    }

    fn on_step_completed(&self, context: &SagaContext, step: &str, duration_millis: u64) {
        tracing::info!(saga_id = %context.saga_id.0, step = %step, duration_ms = duration_millis, "Step completed");
    }

    fn on_step_failed(&self, context: &SagaContext, step: &str, error: &str) {
        tracing::warn!(saga_id = %context.saga_id.0, step = %step, error = %error, "Step failed");
    }

    fn on_saga_quarantined(&self, context: &SagaContext, step: &str, reason: &str) {
        tracing::error!(saga_id = %context.saga_id.0, step = %step, reason = %reason, "Saga quarantined");
    }

    fn on_saga_completed(&self, context: &SagaContext) {
        tracing::info!(saga_id = %context.saga_id.0, "Saga completed");
    }

    fn on_saga_failed(&self, context: &SagaContext, reason: &str) {
        tracing::error!(saga_id = %context.saga_id.0, reason = %reason, "Saga failed");
    }

    fn on_compensation_started(&self, context: &SagaContext, step: &str) {
        tracing::info!(saga_id = %context.saga_id.0, step = %step, "Compensation started");
    }

    fn on_compensation_completed(&self, context: &SagaContext, step: &str) {
        tracing::info!(saga_id = %context.saga_id.0, step = %step, "Compensation completed");
    }
}
