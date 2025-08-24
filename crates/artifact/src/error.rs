use thiserror::Error;

#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("Error repositorio: {0}")] Repository(String),
    #[error("Error almacenamiento: {0}")] Storage(String),
    #[error("Error publicando evento: {0}")] Event(String),
    #[error("Artifact no encontrado")] NotFound,
}

pub type ArtifactResult<T> = Result<T, ArtifactError>;

