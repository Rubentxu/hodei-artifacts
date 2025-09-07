use thiserror::Error;

#[derive(Debug, Error)]
pub enum SupplyChainError {
    #[error("Error repositorio SBOM: {0}")] Repository(String),
    #[error("Error generando SBOM: {0}")] Generation(String),
    #[error("SBOM no encontrado")] NotFound,
}

