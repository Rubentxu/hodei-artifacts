// crates/security/src/infrastructure/errors.rs

use thiserror::Error;

/// Custom error types for the security crate
#[derive(Error, Debug)]
pub enum SecurityError {
    #[error("Policy parse error: {0}")]
    PolicyParseError(String),
    
    #[error("Invalid authorization request: {0}")]
    InvalidRequest(String),
    
    #[error("Cedar engine error: {0}")]
    CedarEngineError(String),
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    
    #[error("Type conversion error: {0}")]
    ConversionError(String),
    
    #[error("Schema error: {0}")]
    SchemaError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Policy validation failed: {0}")]
    PolicyValidationFailed(String),
}