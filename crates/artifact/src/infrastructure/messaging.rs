//! Implementación de `ArtifactEventPublisher` con Kafka (INFRA-T4).

use async_trait::async_trait;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use tracing::error;

use crate::application::ports::ArtifactEventPublisher;
use crate::error::ArtifactError;
use repository::application::ports::EventBus;
use repository::error::RepositoryError;
use shared::domain::event::{DomainEventEnvelope, DomainEventPayload};
use serde::Serialize;

const ARTIFACT_CREATED_TOPIC: &str = "artifact_created";
const ARTIFACT_DOWNLOAD_REQUESTED_TOPIC: &str = "artifact_download_requested";

#[derive(Clone)]
pub struct KafkaArtifactEventPublisher {
    producer: FutureProducer,
}

impl KafkaArtifactEventPublisher {
    pub fn new(brokers: &str) -> Result<Self, ArtifactError> {
        let producer = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .set("message.timeout.ms", "5000")
            .create()
            .map_err(|e| {
                error!("Failed to create Kafka producer: {}", e);
                ArtifactError::Event(e.to_string())
            })?;
        Ok(Self { producer })
    }
}

#[async_trait]
impl ArtifactEventPublisher for KafkaArtifactEventPublisher {
    async fn publish_created(&self, event: &shared::domain::event::ArtifactUploadedEvent) -> Result<(), ArtifactError> {
        let payload = serde_json::to_string(event).map_err(|e| {
            error!("Failed to serialize event: {}", e);
            ArtifactError::EventPublishing { event_type: event.data.full_event_type(), source: Box::new(e) }
        })?;

        let record: FutureRecord<String, String> =
            FutureRecord::to(ARTIFACT_CREATED_TOPIC).payload(&payload);

        self.producer
            .send(record, std::time::Duration::from_secs(0))
            .await
            .map_err(|(e, _)| {
                error!("Failed to send Kafka message: {}", e);
                ArtifactError::EventPublishing { event_type: event.data.full_event_type(), source: Box::new(e) }
            })?;

        Ok(())
    }

    async fn publish_download_requested(&self, event: &shared::domain::event::ArtifactDownloadRequestedEvent) -> Result<(), ArtifactError> {
        let payload = serde_json::to_string(event).map_err(|e| {
            error!("Failed to serialize download event: {}", e);
            ArtifactError::Event(e.to_string())
        })?;

        let record: FutureRecord<String, String> =
            FutureRecord::to(ARTIFACT_DOWNLOAD_REQUESTED_TOPIC).payload(&payload);

        self.producer
            .send(record, std::time::Duration::from_secs(0))
            .await
            .map_err(|(e, _)| {
                error!("Failed to send Kafka download message: {}", e);
                ArtifactError::Event(e.to_string())
            })?;

        Ok(())
    }
}

#[async_trait]
impl EventBus for KafkaArtifactEventPublisher {
    async fn publish<E: DomainEventPayload + Serialize + Send + Sync>(&self, event: &DomainEventEnvelope<E>) -> Result<(), RepositoryError> {
        let payload = serde_json::to_string(event)
            .map_err(|e| RepositoryError::EventPublishing(format!("Failed to serialize event: {}", e)))?;

        let record: FutureRecord<String, String> =
            FutureRecord::to(event.data.base_type()).payload(&payload);

        self.producer
            .send(record, std::time::Duration::from_secs(0))
            .await
            .map_err(|(e, _)| RepositoryError::EventPublishing(format!("Failed to send Kafka message: {}", e)))?;

        Ok(())
    }
}
