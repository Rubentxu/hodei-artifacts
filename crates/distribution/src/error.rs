use thiserror::Error;
#[derive(Debug, Error)]
pub enum DistributionError {
    #[error("Error repositorio distribution: {0}")] Repository(String),
    #[error("Error publicando evento distribution: {0}")] Event(String),
    #[error("Recurso distribution no encontrado")] NotFound,
}
pub type DistributionResult<T> = Result<T, DistributionError>;
