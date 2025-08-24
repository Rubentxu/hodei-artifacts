//! Puertos (traits) compartidos de infraestructura genérica.
use async_trait::async_trait;
use crate::domain::event::ArtifactUploadedEvent;

#[async_trait]
pub trait DomainEventPublisher: Send + Sync {
    async fn publish_artifact_uploaded(&self, event: ArtifactUploadedEvent) -> anyhow::Result<()>;
}

