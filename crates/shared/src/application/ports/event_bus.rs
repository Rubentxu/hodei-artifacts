//! Event Bus abstraction for domain-driven event communication
//!
//! This module provides the core traits and types for implementing an event-driven
//! architecture that enables loosely-coupled communication between bounded contexts.

use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use std::fmt::Debug;
use std::sync::Arc;

/// Marker trait for domain events that can be published through the event bus.
///
/// All domain events must be:
/// - Serializable for transport
/// - Deserializable for consumption
/// - Thread-safe (Send + Sync)
/// - Debuggable for tracing/logging
/// - Static lifetime for storage in collections
pub trait DomainEvent:
    Serialize + DeserializeOwned + Send + Sync + Debug + Clone + 'static
{
    /// Returns the event type identifier for routing and filtering
    fn event_type(&self) -> &'static str;

    /// Returns the aggregate ID that this event relates to (optional)
    fn aggregate_id(&self) -> Option<String> {
        None
    }
}

/// Envelope wrapper for domain events with metadata.
///
/// Provides context about when and why an event occurred, enabling
/// event sourcing, correlation, and causation tracking.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(bound = "T: DomainEvent")]
pub struct EventEnvelope<T: DomainEvent> {
    /// The actual domain event
    pub event: T,

    /// Unique identifier for this event instance
    pub event_id: uuid::Uuid,

    /// Timestamp when the event occurred
    pub occurred_at: chrono::DateTime<chrono::Utc>,

    /// Correlation ID for tracing related events across services
    pub correlation_id: Option<String>,

    /// Causation ID - the ID of the command/event that caused this event
    pub causation_id: Option<String>,

    /// Optional metadata (e.g., user context, tenant ID, etc.)
    pub metadata: std::collections::HashMap<String, String>,
}

impl<T: DomainEvent> EventEnvelope<T> {
    /// Create a new event envelope with default metadata
    pub fn new(event: T) -> Self {
        Self {
            event,
            event_id: uuid::Uuid::new_v4(),
            occurred_at: chrono::Utc::now(),
            correlation_id: None,
            causation_id: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Create an event envelope with correlation tracking
    pub fn with_correlation(event: T, correlation_id: String) -> Self {
        Self {
            event,
            event_id: uuid::Uuid::new_v4(),
            occurred_at: chrono::Utc::now(),
            correlation_id: Some(correlation_id),
            causation_id: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add metadata to the envelope
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Trait for publishing domain events to the event bus.
///
/// Implementations should be non-blocking and handle failures gracefully.
/// Publishing is fire-and-forget; subscribers process events asynchronously.
#[async_trait]
pub trait EventPublisher: Send + Sync {
    /// Publish a domain event to all interested subscribers
    ///
    /// # Errors
    ///
    /// Returns an error if the event cannot be serialized or if the
    /// underlying transport fails critically. Transient failures should
    /// be handled by the implementation (retries, dead letter queues, etc.)
    async fn publish<E: DomainEvent>(&self, event: E) -> anyhow::Result<()>;

    /// Publish an event with explicit envelope metadata
    async fn publish_with_envelope<E: DomainEvent>(
        &self,
        envelope: EventEnvelope<E>,
    ) -> anyhow::Result<()>;
}

/// Handler for processing domain events of a specific type.
///
/// Each handler should be focused on a single responsibility (SRP).
/// Handlers are invoked asynchronously and should be idempotent.
#[async_trait]
pub trait EventHandler<E: DomainEvent>: Send + Sync {
    /// Logical name for this handler (used for tracing and metrics)
    fn name(&self) -> &'static str;

    /// Handle a domain event
    ///
    /// # Errors
    ///
    /// Should return an error if the event cannot be processed.
    /// The bus implementation decides whether to retry, log, or
    /// move to a dead letter queue.
    async fn handle(&self, envelope: EventEnvelope<E>) -> anyhow::Result<()>;

    /// Optional: filter to determine if this handler should process the event
    ///
    /// Default implementation returns true (process all events of this type).
    /// Override to implement more granular filtering.
    fn should_handle(&self, _envelope: &EventEnvelope<E>) -> bool {
        true
    }
}

/// Represents an active subscription to events.
///
/// Subscriptions can be cancelled and provide observability.
pub trait Subscription: Send + Sync {
    /// Unique identifier for this subscription
    fn id(&self) -> &str;

    /// Event type that this subscription listens to
    fn event_type(&self) -> &'static str;

    /// Handler name
    fn handler_name(&self) -> &'static str;

    /// Cancel the subscription (stop receiving events)
    fn cancel(&self);

    /// Check if the subscription is still active
    fn is_active(&self) -> bool;
}

/// Main event bus abstraction.
///
/// Combines publishing and subscription capabilities. Implementations
/// can be in-memory (for monoliths/testing) or distributed (NATS, Kafka, etc.)
#[async_trait]
pub trait EventBus: EventPublisher {
    /// Subscribe a handler to events of a specific type
    ///
    /// The handler will be invoked asynchronously for each event.
    /// Returns a subscription handle that can be used to cancel.
    ///
    /// # Type Parameters
    ///
    /// - `E`: The event type to subscribe to
    /// - `H`: The handler implementation
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription cannot be established.
    async fn subscribe<E, H>(&self, handler: Arc<H>) -> anyhow::Result<Arc<dyn Subscription>>
    where
        E: DomainEvent,
        H: EventHandler<E> + 'static;

    /// Get count of active subscriptions (for monitoring)
    fn subscription_count(&self) -> usize;

    /// Get count of active handlers across all event types
    fn handler_count(&self) -> usize;
}

/// Blanket implementation for Arc-wrapped EventPublisher
#[async_trait]
impl<T: EventPublisher> EventPublisher for Arc<T> {
    async fn publish<E: DomainEvent>(&self, event: E) -> anyhow::Result<()> {
        (**self).publish(event).await
    }

    async fn publish_with_envelope<E: DomainEvent>(
        &self,
        envelope: EventEnvelope<E>,
    ) -> anyhow::Result<()> {
        (**self).publish_with_envelope(envelope).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, serde::Deserialize)]
    struct TestEvent {
        message: String,
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> &'static str {
            "test.event"
        }
    }

    #[test]
    fn test_event_envelope_creation() {
        let event = TestEvent {
            message: "test".to_string(),
        };
        let envelope = EventEnvelope::new(event.clone());

        assert_eq!(envelope.event.message, "test");
        assert!(envelope.correlation_id.is_none());
        assert!(envelope.metadata.is_empty());
    }

    #[test]
    fn test_event_envelope_with_correlation() {
        let event = TestEvent {
            message: "test".to_string(),
        };
        let envelope = EventEnvelope::with_correlation(event, "corr-123".to_string());

        assert_eq!(envelope.correlation_id, Some("corr-123".to_string()));
    }

    #[test]
    fn test_event_envelope_with_metadata() {
        let event = TestEvent {
            message: "test".to_string(),
        };
        let envelope = EventEnvelope::new(event)
            .with_metadata("user_id".to_string(), "user-123".to_string())
            .with_metadata("tenant_id".to_string(), "tenant-456".to_string());

        assert_eq!(envelope.metadata.get("user_id").unwrap(), "user-123");
        assert_eq!(envelope.metadata.get("tenant_id").unwrap(), "tenant-456");
    }
}
