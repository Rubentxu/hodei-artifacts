// crates/distribution/src/domain/maven/mod.rs

//! Dominio Maven - Lógica de negocio pura sin dependencias de infraestructura
//! 
//! Este módulo contiene las entidades, value objects y reglas de negocio
//! específicas del ecosistema Maven. Es completamente síncrono y no tiene
//! dependencias de infraestructura.

pub mod coordinates;
pub mod metadata;
pub mod validation;

// Re-exportar componentes del dominio
pub use coordinates::{MavenCoordinates, MavenValidationError};
pub use metadata::{MavenMetadata, MavenVersion};
pub use validation::{validate_maven_coordinates, validate_maven_version};