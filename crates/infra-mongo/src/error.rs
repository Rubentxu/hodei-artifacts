//! Errores de infraestructura MongoDB.
//!
//! Separado de `client.rs` para mantener Single Responsibility y facilitar tests.
//! Referencia: ver `docs/arquitectura-sistema.md` (Persistencia / Infra).
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MongoInfraError {
    #[error("Error de configuración Mongo: {0}")]
    Config(String),
    #[error("Error de conexión Mongo: {0}")]
    Connection(#[from] mongodb::error::Error),
    #[error("Error genérico infra Mongo: {0}")]
    Other(String),
}

impl From<std::io::Error> for MongoInfraError {
    fn from(e: std::io::Error) -> Self {
        Self::Other(e.to_string())
    }
}