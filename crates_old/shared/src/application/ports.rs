//! Puertos (traits) compartidos de infraestructura genérica.
//!
//! Refactor eventos:
//! - Introducido DomainEventEnvelope<T> + DomainEventPayload.
//! - Método genérico publish<T> para cualquier payload tipado.
//! - Método de conveniencia legacy publish_artifact_uploaded delega al genérico.
use async_trait::async_trait;
use crate::domain::event::{
    DomainEventEnvelope, DomainEventPayload,
    ArtifactUploadedEvent, ArtifactUploaded,
};

#[async_trait]
pub trait DomainEventPublisher: Send + Sync {
    /// Publica un evento envelope genérico.
    async fn publish<T>(&self, envelope: DomainEventEnvelope<T>) -> anyhow::Result<()>
        where T: DomainEventPayload + Send + Sync;

    /// Conveniencia / compatibilidad para ArtifactUploaded.
    async fn publish_artifact_uploaded(&self, event: ArtifactUploadedEvent) -> anyhow::Result<()> {
        // Delegamos al método genérico.
        self.publish::<ArtifactUploaded>(event).await
    }
}

