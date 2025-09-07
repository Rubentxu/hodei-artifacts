use thiserror::Error;
#[derive(Debug, Error)]
pub enum IntegrationError {
    #[error("Error adaptador integration: {0}")] Adapter(String),
    #[error("Recurso integration no encontrado")] NotFound,
}
pub type IntegrationResult<T> = Result<T, IntegrationError>;
