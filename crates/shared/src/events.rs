// crates/shared/src/events.rs

use serde::{Serialize, Deserialize};
use crate::hrn::{Hrn};
use async_trait::async_trait;
use anyhow::Result;

// Nota: Los tipos concretos de eventos (OrganizationEvent, etc.) se definen en sus
// respectivos crates para mantener la cohesión del Bounded Context.
// Este enum actúa como un contenedor universal para el transporte en Kafka.
// use crate::organization::OrganizationEvent;
// use crate::repository::RepositoryEvent;
// use crate::artifact::ArtifactEvent;
// // ... etc.

/// Enumeración de todos los eventos de dominio que pueden fluir por el bus de eventos.
/// Actúa como un sobre que contiene el evento específico de su dominio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DomainEvent {
    // Organization(OrganizationEvent),
    // Repository(RepositoryEvent),
    // Artifact(ArtifactEvent),
    // Iam(IamEvent),
    // Security(SecurityEvent),
    // SupplyChain(SupplyChainEvent),
}

/// Evento de dominio básico
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: Hrn,
    pub r#type: String,
    pub source: String,
    pub timestamp: String, // ISO-8601
    pub data: serde_json::Map<String, serde_json::Value>,
}

/// Flujo de eventos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventStream {
    pub id: EventStreamId,
    pub name: String,
    pub organization: Hrn,
    pub filters: Vec<String>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// Suscripción a un flujo de eventos
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubscription {
    pub id: Hrn,
    pub stream: EventStreamId,
    pub subscriber: String,
    pub active: bool,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// Puerto para publicar eventos de dominio. Implementado en infraestructura.
#[async_trait]
pub trait EventPublisher: Send + Sync {
    async fn publish(&self, event: &DomainEvent) -> anyhow::Result<()>;
}