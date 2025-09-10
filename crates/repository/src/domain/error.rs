// crates/repository/src/domain/error.rs

use shared::hrn::{RepositoryId, OrganizationId};
use thiserror::Error;

/// Errores específicos del dominio de Repository
#[derive(Debug, Clone, Error)]
pub enum RepositoryError {
    #[error("Repository not found: {0}")]
    RepositoryNotFound(String),
    
    #[error("Repository already exists: {0}")]
    RepositoryAlreadyExists(String),
    
    #[error("Organization not found: {0}")]
    OrganizationNotFound(String),
    
    #[error("Invalid repository name: {0}")]
    InvalidRepositoryName(String),
    
    #[error("Invalid repository configuration: {0}")]
    InvalidConfiguration(String),
    
    #[error("Repository type mismatch: expected {expected}, got {actual}")]
    RepositoryTypeMismatch { expected: String, actual: String },
    
    #[error("Referenced repository not found: {0}")]
    ReferencedRepositoryNotFound(String),
    
    #[error("Cannot delete repository with artifacts: {0}")]
    RepositoryNotEmpty(String),
    
    #[error("Storage backend not found: {0}")]
    StorageBackendNotFound(String),
    
    #[error("Unauthorized operation: {0}")]
    Unauthorized(String),
    
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
}

/// Result type para operaciones de Repository
pub type RepositoryResult<T> = Result<T, RepositoryError>;