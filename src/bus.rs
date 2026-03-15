use icanact_core::local_sync::{PubSub, PublishStats, Subscription};

use crate::{
    SagaChoreographyEvent, TerminalPolicy, TerminalResolver, complete_terminal_reply_from_event,
};

#[derive(Clone)]
pub struct SagaChoreographyBus {
    inner: PubSub<SagaChoreographyEvent>,
}

impl SagaChoreographyBus {
    pub fn new() -> Self {
        Self {
            inner: PubSub::new(),
        }
    }

    pub fn subscribe_fn<F>(
        &self,
        topic: &str,
        f: F,
    ) -> Subscription
    where
        F: Fn(SagaChoreographyEvent) -> bool + Send + Sync + 'static,
    {
        self.inner.subscribe_direct_fn(topic, f)
    }

    pub fn unsubscribe(&self, sub: Subscription) -> bool {
        self.inner.unsubscribe(sub)
    }

    pub fn publish(&self, event: SagaChoreographyEvent) -> PublishStats {
        let topic = SagaChoreographyEvent::topic(event.context().saga_type.as_ref());
        self.inner.publish(topic.as_str(), event)
    }

    pub fn publish_to_saga_type(
        &self,
        saga_type: &str,
        event: SagaChoreographyEvent,
    ) -> PublishStats {
        let topic = SagaChoreographyEvent::topic(saga_type);
        self.inner.publish(topic.as_str(), event)
    }

    pub fn subscribe_saga_type_fn<F>(
        &self,
        saga_type: &str,
        f: F,
    ) -> Subscription
    where
        F: Fn(SagaChoreographyEvent) -> bool + Send + Sync + 'static,
    {
        let topic = SagaChoreographyEvent::topic(saga_type);
        self.subscribe_fn(topic.as_str(), f)
    }

    pub fn attach_terminal_resolver(
        &self,
        policy: TerminalPolicy,
        responder: &'static str,
    ) -> Subscription {
        let topic = SagaChoreographyEvent::topic(policy.saga_type.as_ref());
        let resolver = std::sync::Mutex::new(TerminalResolver::new(policy));
        let bus = self.clone();
        self.subscribe_fn(topic.as_str(), move |event: SagaChoreographyEvent| {
            let terminal_events = match resolver.lock() {
                Ok(mut guard) => guard.ingest(&event),
                Err(poisoned) => poisoned.into_inner().ingest(&event),
            };
            for terminal_event in terminal_events {
                let _ = complete_terminal_reply_from_event(&terminal_event, responder);
                let _ = bus.publish(terminal_event);
            }
            true
        })
    }

    pub fn attach_order_lifecycle_terminal_resolver(
        &self,
        responder: &'static str,
    ) -> Subscription {
        self.attach_terminal_resolver(TerminalPolicy::order_lifecycle_default(), responder)
    }
}

impl Default for SagaChoreographyBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::{SagaChoreographyEvent, SagaContext, SagaId, TERMINAL_RESOLVER_STEP};

    use super::SagaChoreographyBus;

    fn context(step_name: &str) -> SagaContext {
        let now = SagaContext::now_millis();
        SagaContext {
            saga_id: SagaId::new(42),
            saga_type: "order_lifecycle".into(),
            step_name: step_name.into(),
            correlation_id: 42,
            causation_id: 42,
            trace_id: 42,
            step_index: 0,
            attempt: 0,
            initiator_peer_id: [0; 32],
            saga_started_at_millis: now,
            event_timestamp_millis: now,
        }
    }

    #[test]
    fn attached_resolver_emits_single_terminal_completion() {
        let bus = SagaChoreographyBus::new();
        let _resolver_sub = bus.attach_order_lifecycle_terminal_resolver("terminal-resolver");

        let terminal_events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let _terminal_capture_sub = bus.subscribe_saga_type_fn("order_lifecycle", {
            let terminal_events = std::sync::Arc::clone(&terminal_events);
            move |event: SagaChoreographyEvent| {
                if matches!(
                    event,
                    SagaChoreographyEvent::SagaCompleted { .. }
                        | SagaChoreographyEvent::SagaFailed { .. }
                        | SagaChoreographyEvent::SagaQuarantined { .. }
                ) {
                    let mut guard = terminal_events.lock().expect("capture lock poisoned");
                    guard.push(event);
                }
                true
            }
        });

        let step = SagaChoreographyEvent::StepCompleted {
            context: context("create_order"),
            output: Vec::new(),
            saga_input: Vec::new(),
            compensation_available: false,
        };
        let _ = bus.publish(step.clone());
        let _ = bus.publish(step);

        let guard = terminal_events.lock().expect("capture lock poisoned");
        assert_eq!(guard.len(), 1, "resolver should latch and emit exactly once");
        assert!(
            matches!(&guard[0], SagaChoreographyEvent::SagaCompleted { context } if context.step_name.as_ref() == TERMINAL_RESOLVER_STEP),
            "resolver terminal event should be emitted from terminal resolver step"
        );
    }
}
