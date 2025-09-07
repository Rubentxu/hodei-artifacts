use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnalyticsError {
    #[error("Error repositorio analytics: {0}")] Repository(String),
    #[error("Error publicando evento analytics: {0}")] Event(String),
    #[error("Recurso analytics no encontrado")] NotFound,
}

pub type AnalyticsResult<T> = Result<T, AnalyticsError>;

