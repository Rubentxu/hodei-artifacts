n use thiserror::Error;

#[derive(Debug, Error)]
pub enum IamError {
    #[error("Error repositorio usuarios: {0}")] Repository(String),
    #[error("Usuario no encontrado")] NotFound,
}

