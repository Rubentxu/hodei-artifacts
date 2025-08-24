//! Implementación de `EventBus` que loguea eventos (INFRA-T4).
//!
//! Para el MVP, es suficiente con una implementación que serialice el evento
//! a JSON y lo emita a la salida estándar (stdout) usando `tracing`.
//! Esto permite verificar el flujo end-to-end sin acoplarse a un broker
//! de mensajería específico (RabbitMQ, Kafka, etc.) en una fase temprana.
//!
//! Futuras extensiones:
//! - Implementación para RabbitMQ con `lapin`.
//! - Implementación para Kafka con `rdkafka`.
//! - Estrategia de reintentos y `dead-letter-queue` (DLQ).

use crate::application::ports::EventBus;
use crate::error::RepositoryError;
use async_trait::async_trait;
use serde::Serialize;
use shared::domain::event::{DomainEventEnvelope, DomainEventPayload};
use tracing::info;

/// Un `EventBus` que simplemente loguea los eventos a `stdout` como JSON.
#[derive(Default, Clone)]
pub struct LoggingEventBus;

impl LoggingEventBus {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl EventBus for LoggingEventBus {
    async fn publish<E>(&self, event: &DomainEventEnvelope<E>) -> Result<(), RepositoryError>
    where
        E: DomainEventPayload + Send + Sync + Serialize,
    {
        let event_json = serde_json::to_string_pretty(event)
            .map_err(|e| RepositoryError::EventPublishing(e.to_string()))?;

        info!(
            event_type = %event.event_type,
            aggregate_id = %event.data.aggregate_id(),
            correlation_id = %event.correlation_id,
            "Publicando evento (logging)"
        );
        // Imprime el JSON a stdout para visibilidad en logs.
        println!("{}", event_json);

        Ok(())
    }
}
