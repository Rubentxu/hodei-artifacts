//! Error types for Get Policy feature

use thiserror::Error;

#[derive(Debug, Error, Clone, PartialEq)]
pub enum GetPolicyError {
    /// La política no fue encontrada
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    /// Error al acceder al repositorio
    #[error("Repository error: {0}")]
    RepositoryError(String),

    /// Error de validación del HRN
    #[error("Invalid HRN: {0}")]
    InvalidHrn(String),
}

