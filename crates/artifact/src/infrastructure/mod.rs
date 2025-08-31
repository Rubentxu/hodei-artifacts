//! Adaptadores de infraestructura (repos, storage, messaging) del bounded context Artifact.
//!
//! Principios:
//! - Implementaciones de puertos definidos en `application::ports`.
//! - No exponer lógica de dominio; sólo adaptación a tecnología (Mongo, S3, Kafka, etc.).
//!
//! Submódulos:
//! - persistence: Impl de `ArtifactRepository` usando MongoDB.
//! - storage: Impl de `ArtifactStorage` usando S3.

pub mod persistence;
pub use persistence::MongoArtifactRepository;

pub mod storage;
pub use storage::S3ArtifactStorage;

pub mod messaging;
pub use messaging::KafkaArtifactEventPublisher;
pub mod rabbitmq_event_publisher;
pub use rabbitmq_event_publisher::RabbitMqArtifactEventPublisher;
