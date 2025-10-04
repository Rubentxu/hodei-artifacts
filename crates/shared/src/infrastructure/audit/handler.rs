//! Generic audit event handler that captures all domain events
//!
//! This handler implements a universal EventHandler that can capture
//! any domain event and store it in the audit log for compliance and debugging.

use super::{AuditLog, AuditLogStore};
use crate::application::ports::event_bus::{DomainEvent, EventEnvelope, EventHandler};
use async_trait::async_trait;
use std::sync::Arc;

/// Universal audit event handler that captures all domain events
///
/// This handler is generic over any DomainEvent type and stores
/// the event data as JSON in the audit log.
pub struct AuditEventHandler {
    store: Arc<AuditLogStore>,
}

impl AuditEventHandler {
    /// Create a new audit event handler with the given store
    pub fn new(store: Arc<AuditLogStore>) -> Self {
        Self { store }
    }

    /// Get the underlying store (useful for testing)
    #[cfg(test)]
    pub fn store(&self) -> Arc<AuditLogStore> {
        self.store.clone()
    }
}

/// Implement EventHandler for any DomainEvent type
///
/// This allows a single AuditEventHandler instance to handle
/// events of different types through dynamic dispatch.
#[async_trait]
impl<E: DomainEvent> EventHandler<E> for AuditEventHandler {
    fn name(&self) -> &'static str {
        "AuditEventHandler"
    }

    async fn handle(&self, envelope: EventEnvelope<E>) -> anyhow::Result<()> {
        // Serialize the event to JSON
        let event_data = serde_json::to_value(&envelope.event)?;

        // Extract aggregate type from metadata
        let aggregate_type = envelope.metadata.get("aggregate_type").cloned();

        // Create audit log entry
        let audit_log = AuditLog {
            id: envelope.event_id,
            event_type: envelope.event.event_type().to_string(),
            aggregate_id: envelope.event.aggregate_id(),
            aggregate_type,
            event_data,
            occurred_at: envelope.occurred_at,
            correlation_id: envelope.correlation_id.clone(),
            causation_id: envelope.causation_id.clone(),
            metadata: envelope.metadata.clone(),
        };

        // Store the audit log
        self.store.add(audit_log.clone()).await;

        // Log to tracing for operational visibility
        tracing::info!(
            event_type = %audit_log.event_type,
            event_id = %audit_log.id,
            aggregate_id = ?audit_log.aggregate_id,
            aggregate_type = ?audit_log.aggregate_type,
            "Domain event captured in audit log"
        );

        Ok(())
    }

    fn should_handle(&self, _envelope: &EventEnvelope<E>) -> bool {
        // Capture ALL events - no filtering
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::ports::event_bus::EventEnvelope;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestEvent {
        message: String,
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> &'static str {
            "test.event"
        }

        fn aggregate_id(&self) -> Option<String> {
            Some("test-123".to_string())
        }
    }

    #[tokio::test]
    async fn test_audit_handler_captures_event() {
        let store = Arc::new(AuditLogStore::new());
        let handler = AuditEventHandler::new(store.clone());

        let event = TestEvent {
            message: "Test message".to_string(),
        };

        let envelope = EventEnvelope::new(event)
            .with_metadata("aggregate_type".to_string(), "TestAggregate".to_string());

        let result = handler.handle(envelope).await;
        assert!(result.is_ok());

        let logs = store.all().await;
        assert_eq!(logs.len(), 1);

        let log = &logs[0];
        assert_eq!(log.event_type, "test.event");
        assert_eq!(log.aggregate_id, Some("test-123".to_string()));
        assert_eq!(log.aggregate_type, Some("TestAggregate".to_string()));
    }

    #[tokio::test]
    async fn test_audit_handler_multiple_events() {
        let store = Arc::new(AuditLogStore::new());
        let handler = AuditEventHandler::new(store.clone());

        for i in 0..5 {
            let event = TestEvent {
                message: format!("Message {}", i),
            };
            let envelope = EventEnvelope::new(event);
            handler.handle(envelope).await.unwrap();
        }

        let logs = store.all().await;
        assert_eq!(logs.len(), 5);
    }

    #[tokio::test]
    async fn test_audit_handler_should_handle_all() {
        let store = Arc::new(AuditLogStore::new());
        let handler = AuditEventHandler::new(store);

        let event = TestEvent {
            message: "Test".to_string(),
        };
        let envelope = EventEnvelope::new(event);

        assert!(handler.should_handle(&envelope));
    }
}
