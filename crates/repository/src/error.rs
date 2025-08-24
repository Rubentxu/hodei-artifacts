use thiserror::Error;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("Error persistencia repositorio: {0}")] Persistence(String),
    #[error("Repositorio no encontrado")] NotFound,
}

