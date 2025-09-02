use thiserror::Error;
use artifact::error::ArtifactError;
use iam::error::IamError;
use cedar_policy::{RequestValidationError, ParseErrors};

#[derive(Debug, Error)]
pub enum DistributionError {
    #[error("Error repositorio distribution: {0}")]
    Repository(String),
    #[error("Error publicando evento distribution: {0}")]
    Event(String),
    #[error("Recurso distribution no encontrado")]
    NotFound,
    #[error("Error de artefacto: {0}")]
    Artifact(#[from] ArtifactError),
    #[error("Error de IAM: {0}")]
    Iam(#[from] IamError),
    #[error("Error de validación de solicitud Cedar: {0}")]
    RequestValidation(#[from] RequestValidationError),
    #[error("Error de parseo Cedar: {0}")]
    CedarParse(#[from] ParseErrors),
    #[error("Error interno: {0}")]
    Internal(String),
    #[error("Paquete npm inválido: {0}")]
    InvalidNpmPackage(String),
}
pub type DistributionResult<T> = Result<T, DistributionError>;
