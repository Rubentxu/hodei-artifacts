// crates/shared/src/events.rs

use serde::{Serialize, Deserialize};

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