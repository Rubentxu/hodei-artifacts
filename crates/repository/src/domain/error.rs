
// crates/repository/src/domain/error.rs

use shared::hrn::HrnError;
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
    
    #[error("Repository type mismatch")]
    RepositoryTypeMismatch,
    
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

    #[error("HRN error: {0}")]
    HrnError(String),
}

impl From<HrnError> for RepositoryError {
    fn from(err: HrnError) -> Self {
        RepositoryError::HrnError(err.to_string())
    }
}

impl From<mongodb::error::Error> for RepositoryError {
    fn from(err: mongodb::error::Error) -> Self {
        RepositoryError::DatabaseError(err.to_string())
    }
}

impl From<mongodb::bson::document::ValueAccessError> for RepositoryError {
    fn from(err: mongodb::bson::document::ValueAccessError) -> Self {
        RepositoryError::DatabaseError(err.to_string())
    }
}

impl From<url::ParseError> for RepositoryError {
    fn from(err: url::ParseError) -> Self {
        RepositoryError::InvalidConfiguration(err.to_string())
    }
}



/// Result type para operaciones de Repository
pub type RepositoryResult<T> = Result<T, RepositoryError>;
