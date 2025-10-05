//! In-memory event bus implementation using tokio broadcast channels
//!
//! This implementation is suitable for:
//! - Monolithic deployments
//! - Development and testing
//! - Local event-driven architectures
//!
//! For distributed systems, use a message broker adapter (NATS, Kafka, etc.)

use crate::application::ports::event_bus::{
    DomainEvent, EventBus, EventEnvelope, EventHandler, EventPublisher, Subscription,
};
use async_trait::async_trait;
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

/// Internal representation of a channel for a specific event type
struct TypedChannel {
    sender: broadcast::Sender<Vec<u8>>,
}

/// In-memory event bus using tokio broadcast channels
///
/// Each event type gets its own broadcast channel. Handlers subscribe
/// to specific event types and receive events asynchronously via spawned tasks.
///
/// # Performance Characteristics
///
/// - Publishing: O(1) - just sends to broadcast channel
/// - Fan-out: Automatic via broadcast (each subscriber gets a copy)
/// - Lagging: Subscribers that can't keep up will skip events (logged as warning)
/// - Memory: Bounded channel size (default 1024 events per type)
///
/// # Thread Safety
///
/// All operations are thread-safe and can be called from multiple tasks concurrently.
pub struct InMemoryEventBus {
    /// Map of TypeId -> broadcast channel for each event type
    channels: RwLock<HashMap<TypeId, TypedChannel>>,

    /// Active subscriptions count (for monitoring)
    subscription_count: Arc<std::sync::atomic::AtomicUsize>,

    /// Channel capacity per event type
    channel_capacity: usize,
}

impl InMemoryEventBus {
    /// Create a new in-memory event bus with default capacity (1024)
    pub fn new() -> Self {
        Self::with_capacity(1024)
    }

    /// Create a new in-memory event bus with specified channel capacity
    ///
    /// # Arguments
    ///
    /// * `capacity` - Number of events to buffer per event type channel
    ///
    /// # Recommendations
    ///
    /// - For high-throughput: 2048 or higher
    /// - For low-latency: 256 or lower
    /// - For testing: 16 (makes lag scenarios easier to trigger)
    pub fn with_capacity(capacity: usize) -> Self {
        info!("Creating InMemoryEventBus with capacity {}", capacity);
        Self {
            channels: RwLock::new(HashMap::new()),
            subscription_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            channel_capacity: capacity,
        }
    }

    /// Get or create a broadcast channel for a specific event type
    fn get_or_create_channel<E: DomainEvent>(&self) -> broadcast::Sender<Vec<u8>> {
        let type_id = TypeId::of::<E>();

        // Fast path: channel already exists
        {
            let channels = self.channels.read().unwrap();
            if let Some(channel) = channels.get(&type_id) {
                return channel.sender.clone();
            }
        }

        // Slow path: create new channel
        let mut channels = self.channels.write().unwrap();

        // Double-check in case another thread created it
        if let Some(channel) = channels.get(&type_id) {
            return channel.sender.clone();
        }

        let (tx, _rx) = broadcast::channel::<Vec<u8>>(self.channel_capacity);
        let event_type = std::any::type_name::<E>();

        debug!(
            "Created new broadcast channel for event type: {}",
            event_type
        );

        channels.insert(type_id, TypedChannel { sender: tx.clone() });

        tx
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventPublisher for InMemoryEventBus {
    async fn publish<E: DomainEvent>(&self, event: E) -> anyhow::Result<()> {
        let envelope = EventEnvelope::new(event);
        self.publish_with_envelope(envelope).await
    }

    async fn publish_with_envelope<E: DomainEvent>(
        &self,
        envelope: EventEnvelope<E>,
    ) -> anyhow::Result<()> {
        let event_type = envelope.event.event_type();

        debug!(
            event_type = event_type,
            event_id = %envelope.event_id,
            "Publishing event"
        );

        // Serialize the envelope
        let bytes = bincode::serialize(&envelope)
            .map_err(|e| anyhow::anyhow!("Failed to serialize event envelope: {}", e))?;

        // Get the channel and send
        let sender = self.get_or_create_channel::<E>();
        let receiver_count = sender.receiver_count();

        if receiver_count == 0 {
            debug!(
                event_type = event_type,
                "No subscribers for event type, event will be dropped"
            );
        }

        // Send returns error only if there are no receivers (which is fine)
        let _ = sender.send(bytes);

        debug!(
            event_type = event_type,
            event_id = %envelope.event_id,
            receivers = receiver_count,
            "Event published"
        );

        Ok(())
    }
}

#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn subscribe<E, H>(&self, handler: Arc<H>) -> anyhow::Result<Arc<dyn Subscription>>
    where
        E: DomainEvent,
        H: EventHandler<E> + 'static,
    {
        let sender = self.get_or_create_channel::<E>();
        let mut receiver = sender.subscribe();
        let handler_name = handler.name();
        let event_type_name = std::any::type_name::<E>();

        info!(
            handler = handler_name,
            event_type = event_type_name,
            "Subscribing handler to event type"
        );

        let (cancel_tx, mut cancel_rx) = tokio::sync::oneshot::channel::<()>();
        let subscription_id = format!("{}-{}", handler_name, uuid::Uuid::new_v4());
        let is_active = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let is_active_clone = is_active.clone();

        // Increment subscription count
        self.subscription_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let sub_count_clone = self.subscription_count.clone();

        // Spawn task to handle incoming events
        let task: JoinHandle<()> = tokio::spawn(async move {
            let mut processed_count = 0u64;
            let mut error_count = 0u64;
            let mut lagged_count = 0u64;

            loop {
                tokio::select! {
                    biased;

                    // Check for cancellation first
                    _ = &mut cancel_rx => {
                        info!(
                            handler = handler_name,
                            processed = processed_count,
                            errors = error_count,
                            lagged = lagged_count,
                            "Handler subscription cancelled"
                        );
                        break;
                    }

                    // Receive event
                    msg = receiver.recv() => {
                        match msg {
                            Ok(bytes) => {
                                // Deserialize envelope
                                match bincode::deserialize::<EventEnvelope<E>>(&bytes) {
                                    Ok(envelope) => {
                                        // Check if handler wants to process this event
                                        if !handler.should_handle(&envelope) {
                                            debug!(
                                                handler = handler_name,
                                                event_id = %envelope.event_id,
                                                "Handler filtered out event"
                                            );
                                            continue;
                                        }

                                        // Handle the event
                                        match handler.handle(envelope.clone()).await {
                                            Ok(_) => {
                                                processed_count += 1;
                                                debug!(
                                                    handler = handler_name,
                                                    event_id = %envelope.event_id,
                                                    processed = processed_count,
                                                    "Event handled successfully"
                                                );
                                            }
                                            Err(e) => {
                                                error_count += 1;
                                                error!(
                                                    handler = handler_name,
                                                    event_id = %envelope.event_id,
                                                    error = %e,
                                                    errors = error_count,
                                                    "Handler failed to process event"
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error_count += 1;
                                        error!(
                                            handler = handler_name,
                                            error = %e,
                                            "Failed to deserialize event envelope"
                                        );
                                    }
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                lagged_count += skipped;
                                warn!(
                                    handler = handler_name,
                                    skipped = skipped,
                                    total_lagged = lagged_count,
                                    "Handler lagged behind, events were skipped"
                                );
                            }
                            Err(broadcast::error::RecvError::Closed) => {
                                info!(
                                    handler = handler_name,
                                    "Event channel closed, stopping handler"
                                );
                                break;
                            }
                        }
                    }
                }
            }

            // Mark as inactive when task completes
            is_active_clone.store(false, std::sync::atomic::Ordering::Relaxed);
            sub_count_clone.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        });

        // Create subscription handle
        let subscription = Arc::new(InMemorySubscription {
            id: subscription_id,
            event_type: event_type_name,
            handler_name,
            cancel_tx: tokio::sync::Mutex::new(Some(cancel_tx)),
            is_active,
            _task: task,
        });

        Ok(subscription as Arc<dyn Subscription>)
    }

    fn subscription_count(&self) -> usize {
        self.subscription_count
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn handler_count(&self) -> usize {
        self.subscription_count()
    }
}

/// Implementation of Subscription for in-memory subscriptions
struct InMemorySubscription {
    id: String,
    event_type: &'static str,
    handler_name: &'static str,
    cancel_tx: tokio::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    is_active: Arc<std::sync::atomic::AtomicBool>,
    _task: JoinHandle<()>,
}

impl Subscription for InMemorySubscription {
    fn id(&self) -> &str {
        &self.id
    }

    fn event_type(&self) -> &'static str {
        self.event_type
    }

    fn handler_name(&self) -> &'static str {
        self.handler_name
    }

    fn cancel(&self) {
        info!(
            subscription_id = self.id,
            handler = self.handler_name,
            "Cancelling subscription"
        );

        // Try to acquire lock without blocking
        if let Ok(mut guard) = self.cancel_tx.try_lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(());
                self.is_active
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
        } else {
            // If we can't get the lock, mark as inactive anyway
            self.is_active
                .store(false, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn is_active(&self) -> bool {
        self.is_active.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
    struct TestEvent {
        message: String,
    }

    impl DomainEvent for TestEvent {
        fn event_type(&self) -> &'static str {
            "test.event"
        }
    }

    struct TestHandler {
        name: &'static str,
        counter: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EventHandler<TestEvent> for TestHandler {
        fn name(&self) -> &'static str {
            self.name
        }

        async fn handle(&self, envelope: EventEnvelope<TestEvent>) -> anyhow::Result<()> {
            self.counter.fetch_add(1, Ordering::SeqCst);
            tracing::info!("Handled event: {}", envelope.event.message);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_publish_and_subscribe() {
        let bus = InMemoryEventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let handler = Arc::new(TestHandler {
            name: "test_handler",
            counter: counter.clone(),
        });

        let _subscription = bus.subscribe::<TestEvent, _>(handler).await.unwrap();

        // Give handler time to set up
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Publish event
        bus.publish(TestEvent {
            message: "Hello".to_string(),
        })
        .await
        .unwrap();

        // Give handler time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_multiple_handlers() {
        let bus = InMemoryEventBus::new();
        let counter1 = Arc::new(AtomicUsize::new(0));
        let counter2 = Arc::new(AtomicUsize::new(0));

        let handler1 = Arc::new(TestHandler {
            name: "handler_1",
            counter: counter1.clone(),
        });
        let handler2 = Arc::new(TestHandler {
            name: "handler_2",
            counter: counter2.clone(),
        });

        let _sub1 = bus.subscribe::<TestEvent, _>(handler1).await.unwrap();
        let _sub2 = bus.subscribe::<TestEvent, _>(handler2).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        bus.publish(TestEvent {
            message: "Broadcast".to_string(),
        })
        .await
        .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        assert_eq!(counter1.load(Ordering::SeqCst), 1);
        assert_eq!(counter2.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_subscription_cancel() {
        let bus = InMemoryEventBus::new();
        let counter = Arc::new(AtomicUsize::new(0));

        let handler = Arc::new(TestHandler {
            name: "cancellable",
            counter: counter.clone(),
        });

        let subscription = bus.subscribe::<TestEvent, _>(handler).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Cancel subscription
        subscription.cancel();

        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Publish after cancel
        bus.publish(TestEvent {
            message: "After cancel".to_string(),
        })
        .await
        .unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Should not have processed
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        assert!(!subscription.is_active());
    }

    #[tokio::test]
    async fn test_publish_without_subscribers() {
        let bus = InMemoryEventBus::new();

        // Should not error even with no subscribers
        let result = bus
            .publish(TestEvent {
                message: "No one listening".to_string(),
            })
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_subscription_count() {
        let bus = InMemoryEventBus::new();
        assert_eq!(bus.subscription_count(), 0);

        let counter = Arc::new(AtomicUsize::new(0));
        let handler = Arc::new(TestHandler {
            name: "counter_test",
            counter: counter.clone(),
        });

        let sub = bus.subscribe::<TestEvent, _>(handler).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        assert_eq!(bus.subscription_count(), 1);

        sub.cancel();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        assert_eq!(bus.subscription_count(), 0);
    }
}
