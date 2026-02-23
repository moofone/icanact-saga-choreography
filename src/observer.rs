//! Saga observer trait

use super::SagaContext;

/// Observer trait for external observability
pub trait SagaObserver: Send + Sync + 'static {
    fn on_saga_started(&self, context: &SagaContext);
    fn on_step_started(&self, context: &SagaContext, step: &str);
    fn on_step_completed(&self, context: &SagaContext, step: &str, duration_millis: u64);
    fn on_step_failed(&self, context: &SagaContext, step: &str, error: &str);
    fn on_compensation_started(&self, context: &SagaContext, step: &str);
    fn on_compensation_completed(&self, context: &SagaContext, step: &str);
    fn on_saga_completed(&self, context: &SagaContext);
    fn on_saga_failed(&self, context: &SagaContext, reason: &str);
    fn on_saga_quarantined(&self, context: &SagaContext, step: &str, reason: &str);
}

/// No-op observer
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

/// Tracing-based observer
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
