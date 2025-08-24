use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("Error persistencia repositorio: {0}")]
    Persistence(String),
    #[error("Repositorio no encontrado")]
    NotFound,
    #[error("Nombre de repositorio duplicado")]
    DuplicateName,
    #[error("Input inválido: {0}")]
    InvalidInput(String),
    #[error("Error publicando evento: {0}")]
    EventPublishing(String),
}
