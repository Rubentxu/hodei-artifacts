//! Infrastructure-level errors for distribution

use thiserror::Error;

#[derive(Error, Debug)]
pub enum DistributionInfrastructureError {
    #[error("S3 error: {0}")]
    S3Error(String),
    
    #[error("MongoDB error: {0}")]
    MongoDbError(String),
    
    #[error("Redis error: {0}")]
    RedisError(String),
    
    #[error("Cedar error: {0}")]
    CedarError(String),
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Conflict: {0}")]
    Conflict(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

impl DistributionInfrastructureError {
    pub fn to_http_status(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        
        match self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::PermissionDenied(_) => StatusCode::FORBIDDEN,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::TimeoutError(_) => StatusCode::REQUEST_TIMEOUT,
            Self::S3Error(_) | Self::MongoDbError(_) | Self::RedisError(_) | Self::CedarError(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
            Self::ConfigurationError(_) | Self::SerializationError(_) | Self::NetworkError(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
            Self::IoError(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::InternalError(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

pub type Result<T> = std::result::Result<T, DistributionInfrastructureError>;