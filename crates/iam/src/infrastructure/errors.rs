// crates/iam/src/infrastructure/errors.rs

use thiserror::Error;
use cedar_policy::PolicyId;
use crate::domain::policy::PolicyStatus;

/// Validation error details for policy content
#[derive(Debug, Clone, PartialEq)]
pub struct ValidationError {
    pub message: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

/// Custom error types for the IAM crate
#[derive(Error, Debug)]
pub enum IamError {
    #[error("Policy not found: {0:?}")]
    PolicyNotFound(PolicyId),
    
    #[error("Policy validation failed")]
    PolicyValidationFailed { errors: Vec<ValidationError> },
    
    #[error("Invalid policy status transition from {from:?} to {to:?}")]
    InvalidStatusTransition { from: PolicyStatus, to: PolicyStatus },
    
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    
    #[error("Database error: {0}")]
    DatabaseError(String),
    
    #[error("Authorization error: {0}")]
    AuthorizationError(String),
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    
    #[error("Policy already exists: {0:?}")]
    PolicyAlreadyExists(PolicyId),
    
    #[error("Concurrent modification detected for policy: {0:?}")]
    ConcurrentModification(PolicyId),
    
    #[error("Policy is read-only and cannot be modified: {0:?}")]
    PolicyReadOnly(PolicyId),
    
    #[error("Internal error: {0}")]
    InternalError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Timeout error: {0}")]
    TimeoutError(String),
    
    #[error("Event publish error: {0}")]
    EventPublishError(String),
}

impl IamError {
    /// Create a database error from a MongoDB error
    pub fn from_mongodb_error(error: mongodb::error::Error) -> Self {
        Self::DatabaseError(error.to_string())
    }

    /// Create a validation error from a single message
    pub fn validation_error(message: String) -> Self {
        Self::PolicyValidationFailed {
            errors: vec![ValidationError {
                message,
                line: None,
                column: None,
            }],
        }
    }

    /// Create a validation error with line and column information
    pub fn validation_error_with_location(message: String, line: u32, column: u32) -> Self {
        Self::PolicyValidationFailed {
            errors: vec![ValidationError {
                message,
                line: Some(line),
                column: Some(column),
            }],
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            IamError::NetworkError(_) | IamError::TimeoutError(_) | IamError::InternalError(_)
        )
    }

    /// Check if this error is a client error (4xx)
    pub fn is_client_error(&self) -> bool {
        matches!(
            self,
            IamError::PolicyNotFound(_)
                | IamError::PolicyValidationFailed { .. }
                | IamError::InvalidInput(_)
                | IamError::InvalidStatusTransition { .. }
                | IamError::PolicyAlreadyExists(_)
                | IamError::PolicyReadOnly(_)
                | IamError::AuthorizationError(_)
        )
    }

    /// Check if this error is a server error (5xx)
    pub fn is_server_error(&self) -> bool {
        matches!(
            self,
            IamError::DatabaseError(_)
                | IamError::ConfigurationError(_)
                | IamError::InternalError(_)
                | IamError::SerializationError(_)
                | IamError::NetworkError(_)
                | IamError::TimeoutError(_)
        )
    }

    /// Get HTTP status code for this error
    pub fn http_status_code(&self) -> u16 {
        match self {
            IamError::PolicyNotFound(_) => 404,
            IamError::PolicyValidationFailed { .. } => 400,
            IamError::InvalidInput(_) => 400,
            IamError::InvalidStatusTransition { .. } => 400,
            IamError::PolicyAlreadyExists(_) => 409,
            IamError::AuthorizationError(_) => 403,
            IamError::PolicyReadOnly(_) => 403,
            IamError::ConcurrentModification(_) => 409,
            IamError::DatabaseError(_) => 500,
            IamError::ConfigurationError(_) => 500,
            IamError::InternalError(_) => 500,
            IamError::SerializationError(_) => 500,
            IamError::NetworkError(_) => 502,
            IamError::TimeoutError(_) => 504,
            IamError::EventPublishError(_) => 500,
        }
    }

    /// Get error code for programmatic handling
    pub fn error_code(&self) -> &'static str {
        match self {
            IamError::PolicyNotFound(_) => "POLICY_NOT_FOUND",
            IamError::PolicyValidationFailed { .. } => "POLICY_VALIDATION_FAILED",
            IamError::InvalidInput(_) => "INVALID_INPUT",
            IamError::InvalidStatusTransition { .. } => "INVALID_STATUS_TRANSITION",
            IamError::PolicyAlreadyExists(_) => "POLICY_ALREADY_EXISTS",
            IamError::AuthorizationError(_) => "AUTHORIZATION_ERROR",
            IamError::PolicyReadOnly(_) => "POLICY_READ_ONLY",
            IamError::ConcurrentModification(_) => "CONCURRENT_MODIFICATION",
            IamError::DatabaseError(_) => "DATABASE_ERROR",
            IamError::ConfigurationError(_) => "CONFIGURATION_ERROR",
            IamError::InternalError(_) => "INTERNAL_ERROR",
            IamError::SerializationError(_) => "SERIALIZATION_ERROR",
            IamError::NetworkError(_) => "NETWORK_ERROR",
            IamError::TimeoutError(_) => "TIMEOUT_ERROR",
            IamError::EventPublishError(_) => "EVENT_PUBLISH_ERROR",
        }
    }
}

// Conversion from domain policy errors
impl From<crate::domain::policy::PolicyError> for IamError {
    fn from(error: crate::domain::policy::PolicyError) -> Self {
        match error {
            crate::domain::policy::PolicyError::InvalidStatusTransition { from, to } => {
                IamError::InvalidStatusTransition { from, to }
            }
            crate::domain::policy::PolicyError::InvalidName(msg) => IamError::InvalidInput(msg),
            crate::domain::policy::PolicyError::InvalidContent(msg) => IamError::InvalidInput(msg),
            crate::domain::policy::PolicyError::InvalidVersion => {
                IamError::InvalidInput("Invalid policy version".to_string())
            }
        }
    }
}

// Conversion from MongoDB errors
impl From<mongodb::error::Error> for IamError {
    fn from(error: mongodb::error::Error) -> Self {
        IamError::from_mongodb_error(error)
    }
}

// Conversion from serde_json errors
impl From<serde_json::Error> for IamError {
    fn from(error: serde_json::Error) -> Self {
        IamError::SerializationError(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::{Hrn, PolicyId};

    fn create_test_policy_id() -> PolicyId {
        PolicyId(Hrn::new("hrn:hodei:iam:global:org_123:policy/test_policy").expect("Valid HRN"))
    }

    #[test]
    fn test_error_display() {
        let policy_id = create_test_policy_id();
        
        let error = IamError::PolicyNotFound(policy_id.clone());
        assert!(error.to_string().contains("Policy not found"));
        
        let error = IamError::InvalidInput("Test message".to_string());
        assert!(error.to_string().contains("Invalid input"));
        assert!(error.to_string().contains("Test message"));
        
        let error = IamError::DatabaseError("Connection failed".to_string());
        assert!(error.to_string().contains("Database error"));
        assert!(error.to_string().contains("Connection failed"));
    }

    #[test]
    fn test_validation_error_creation() {
        let error = IamError::validation_error("Invalid syntax".to_string());
        match error {
            IamError::PolicyValidationFailed { errors } => {
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].message, "Invalid syntax");
                assert_eq!(errors[0].line, None);
                assert_eq!(errors[0].column, None);
            }
            _ => panic!("Expected PolicyValidationFailed error"),
        }
    }

    #[test]
    fn test_validation_error_with_location() {
        let error = IamError::validation_error_with_location("Syntax error".to_string(), 5, 10);
        match error {
            IamError::PolicyValidationFailed { errors } => {
                assert_eq!(errors.len(), 1);
                assert_eq!(errors[0].message, "Syntax error");
                assert_eq!(errors[0].line, Some(5));
                assert_eq!(errors[0].column, Some(10));
            }
            _ => panic!("Expected PolicyValidationFailed error"),
        }
    }

    #[test]
    fn test_error_classification() {
        let policy_id = create_test_policy_id();
        
        // Client errors
        assert!(IamError::PolicyNotFound(policy_id.clone()).is_client_error());
        assert!(IamError::InvalidInput("test".to_string()).is_client_error());
        assert!(IamError::AuthorizationError("test".to_string()).is_client_error());
        
        // Server errors
        assert!(IamError::DatabaseError("test".to_string()).is_server_error());
        assert!(IamError::InternalError("test".to_string()).is_server_error());
        assert!(IamError::ConfigurationError("test".to_string()).is_server_error());
        
        // Retryable errors
        assert!(IamError::NetworkError("test".to_string()).is_retryable());
        assert!(IamError::TimeoutError("test".to_string()).is_retryable());
        assert!(IamError::InternalError("test".to_string()).is_retryable());
        
        // Non-retryable errors
        assert!(!IamError::PolicyNotFound(policy_id).is_retryable());
        assert!(!IamError::InvalidInput("test".to_string()).is_retryable());
    }

    #[test]
    fn test_http_status_codes() {
        let policy_id = create_test_policy_id();
        
        assert_eq!(IamError::PolicyNotFound(policy_id.clone()).http_status_code(), 404);
        assert_eq!(IamError::InvalidInput("test".to_string()).http_status_code(), 400);
        assert_eq!(IamError::AuthorizationError("test".to_string()).http_status_code(), 403);
        assert_eq!(IamError::PolicyAlreadyExists(policy_id).http_status_code(), 409);
        assert_eq!(IamError::DatabaseError("test".to_string()).http_status_code(), 500);
        assert_eq!(IamError::NetworkError("test".to_string()).http_status_code(), 502);
        assert_eq!(IamError::TimeoutError("test".to_string()).http_status_code(), 504);
    }

    #[test]
    fn test_error_codes() {
        let policy_id = create_test_policy_id();
        
        assert_eq!(IamError::PolicyNotFound(policy_id).error_code(), "POLICY_NOT_FOUND");
        assert_eq!(IamError::InvalidInput("test".to_string()).error_code(), "INVALID_INPUT");
        assert_eq!(IamError::DatabaseError("test".to_string()).error_code(), "DATABASE_ERROR");
        assert_eq!(IamError::AuthorizationError("test".to_string()).error_code(), "AUTHORIZATION_ERROR");
    }

    #[test]
    fn test_conversion_from_policy_error() {
        let policy_error = crate::domain::policy::PolicyError::InvalidName("Test".to_string());
        let iam_error: IamError = policy_error.into();
        
        match iam_error {
            IamError::InvalidInput(msg) => assert_eq!(msg, "Test"),
            _ => panic!("Expected InvalidInput error"),
        }
    }

    #[test]
    fn test_conversion_from_status_transition_error() {
        let policy_error = crate::domain::policy::PolicyError::InvalidStatusTransition {
            from: PolicyStatus::Deprecated,
            to: PolicyStatus::Active,
        };
        let iam_error: IamError = policy_error.into();
        
        match iam_error {
            IamError::InvalidStatusTransition { from, to } => {
                assert_eq!(from, PolicyStatus::Deprecated);
                assert_eq!(to, PolicyStatus::Active);
            }
            _ => panic!("Expected InvalidStatusTransition error"),
        }
    }
}