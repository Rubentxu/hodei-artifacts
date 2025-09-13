//! Error types for Get Organization Details Feature
//!
//! This module defines all error types used throughout the organization retrieval feature.

use thiserror::Error;

/// Organization-specific errors
#[derive(Debug, Error)]
pub enum OrganizationError {
    #[error("Organization not found: {0}")]
    OrganizationNotFound(String),
    
    #[error("Access denied to organization: {0}")]
    AccessDenied(String),
    
    #[error("Invalid organization ID: {0}")]
    InvalidOrganizationId(String),
    
    #[error("Organization is suspended: {0}")]
    OrganizationSuspended(String),
    
    #[error("Organization is archived: {0}")]
    OrganizationArchived(String),
    
    #[error("Member not found: {0}")]
    MemberNotFound(String),
    
    #[error("Invitation not found: {0}")]
    InvitationNotFound(String),
    
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),
    
    #[error("Validation error: {0}")]
    Validation(String),
    
    #[error("Database error: {0}")]
    Database(String),
    
    #[error("Internal server error: {0}")]
    Internal(String),
}

impl OrganizationError {
    /// Create organization not found error
    pub fn organization_not_found<S: Into<String>>(id: S) -> Self {
        Self::OrganizationNotFound(id.into())
    }
    
    /// Create access denied error
    pub fn access_denied<S: Into<String>>(reason: S) -> Self {
        Self::AccessDenied(reason.into())
    }
    
    /// Create invalid organization ID error
    pub fn invalid_organization_id<S: Into<String>>(id: S) -> Self {
        Self::InvalidOrganizationId(id.into())
    }
    
    /// Create organization suspended error
    pub fn organization_suspended<S: Into<String>>(id: S) -> Self {
        Self::OrganizationSuspended(id.into())
    }
    
    /// Create organization archived error
    pub fn organization_archived<S: Into<String>>(id: S) -> Self {
        Self::OrganizationArchived(id.into())
    }
    
    /// Create validation error
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::Validation(message.into())
    }
    
    /// Create database error
    pub fn database<S: Into<String>>(message: S) -> Self {
        Self::Database(message.into())
    }
    
    /// Create internal error
    pub fn internal<S: Into<String>>(message: S) -> Self {
        Self::Internal(message.into())
    }
    
    /// Check if error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(self, OrganizationError::Database(_))
    }
    
    /// Get HTTP status code
    pub fn status_code(&self) -> u16 {
        match self {
            OrganizationError::OrganizationNotFound(_) => 404,
            OrganizationError::MemberNotFound(_) => 404,
            OrganizationError::InvitationNotFound(_) => 404,
            OrganizationError::PolicyNotFound(_) => 404,
            OrganizationError::AccessDenied(_) => 403,
            OrganizationError::OrganizationSuspended(_) => 403,
            OrganizationError::OrganizationArchived(_) => 410,
            OrganizationError::InvalidOrganizationId(_) => 400,
            OrganizationError::Validation(_) => 400,
            OrganizationError::Database(_) => 503,
            OrganizationError::Internal(_) => 500,
        }
    }
    
    /// Get error category
    pub fn category(&self) -> ErrorCategory {
        match self {
            OrganizationError::OrganizationNotFound(_) => ErrorCategory::NotFound,
            OrganizationError::MemberNotFound(_) => ErrorCategory::NotFound,
            OrganizationError::InvitationNotFound(_) => ErrorCategory::NotFound,
            OrganizationError::PolicyNotFound(_) => ErrorCategory::NotFound,
            OrganizationError::AccessDenied(_) => ErrorCategory::Permission,
            OrganizationError::OrganizationSuspended(_) => ErrorCategory::Permission,
            OrganizationError::OrganizationArchived(_) => ErrorCategory::Permission,
            OrganizationError::InvalidOrganizationId(_) => ErrorCategory::Validation,
            OrganizationError::Validation(_) => ErrorCategory::Validation,
            OrganizationError::Database(_) => ErrorCategory::Database,
            OrganizationError::Internal(_) => ErrorCategory::Internal,
        }
    }
}

/// Error categories
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCategory {
    NotFound,
    Permission,
    Validation,
    Database,
    Internal,
}

/// Result type for organization operations
pub type OrganizationResult<T> = Result<T, OrganizationError>;

/// Health check result for repository
#[derive(Debug, Clone)]
pub struct RepositoryHealth {
    pub is_healthy: bool,
    pub status: shared::lifecycle::HealthStatus,
    pub message: String,
}

impl RepositoryHealth {
    pub fn new(is_healthy: bool, status: shared::lifecycle::HealthStatus, message: String) -> Self {
        Self {
            is_healthy,
            status,
            message,
        }
    }
    
    pub fn healthy() -> Self {
        Self::new(
            true,
            shared::lifecycle::HealthStatus::Healthy,
            "Repository is healthy".to_string(),
        )
    }
    
    pub fn unhealthy(message: String) -> Self {
        Self::new(
            false,
            shared::lifecycle::HealthStatus::Unhealthy,
            message,
        )
    }
    
    pub fn is_healthy(&self) -> bool {
        self.is_healthy
    }
    
    pub fn status(&self) -> shared::lifecycle::HealthStatus {
        self.status.clone()
    }
    
    pub fn message(&self) -> String {
        self.message.clone()
    }
}

impl Default for RepositoryHealth {
    fn default() -> Self {
        Self::healthy()
    }
}

/// Extension trait for adding context to results
pub trait ResultExt<T> {
    fn with_context<C: Into<String>, O: Into<String>>(
        self,
        operation: O,
        component: C,
    ) -> Result<T, ContextualError>;
}

impl<T> ResultExt<T> for OrganizationResult<T> {
    fn with_context<C: Into<String>, O: Into<String>>(
        self,
        operation: O,
        component: C,
    ) -> Result<T, ContextualError> {
        self.map_err(|error| ContextualError::new(
            error,
            ErrorContext::new(operation, component),
        ))
    }
}

/// Error context for better debugging
#[derive(Debug, Clone)]
pub struct ErrorContext {
    pub operation: String,
    pub component: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub metadata: std::collections::HashMap<String, String>,
}

impl ErrorContext {
    pub fn new<S: Into<String>>(operation: S, component: S) -> Self {
        Self {
            operation: operation.into(),
            component: component.into(),
            timestamp: chrono::Utc::now(),
            metadata: std::collections::HashMap::new(),
        }
    }
    
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }
}

/// Enhanced error with context
#[derive(Debug, Error)]
pub struct ContextualError {
    #[source]
    pub error: OrganizationError,
    pub context: ErrorContext,
}

impl ContextualError {
    pub fn new(error: OrganizationError, context: ErrorContext) -> Self {
        Self { error, context }
    }
}

impl std::fmt::Display for ContextualError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{} @ {}] {}",
            self.context.operation, self.context.component, self.error
        )
    }
}

impl From<OrganizationError> for ContextualError {
    fn from(error: OrganizationError) -> Self {
        Self {
            error,
            context: ErrorContext::new("unknown", "unknown"),
        }
    }
}

/// Result type with contextual error
pub type ContextualResult<T> = Result<T, ContextualError>;

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_creation() {
        let error = OrganizationError::organization_not_found("test-org");
        assert_eq!(error.status_code(), 404);
        assert_eq!(error.category(), ErrorCategory::NotFound);
        
        let error = OrganizationError::validation("Invalid input");
        assert_eq!(error.status_code(), 400);
        assert_eq!(error.category(), ErrorCategory::Validation);
    }
    
    #[test]
    fn test_error_retryable() {
        assert!(OrganizationError::database("Connection failed").is_retryable());
        assert!(!OrganizationError::validation("Invalid input").is_retryable());
    }
    
    #[test]
    fn test_repository_health() {
        let health = RepositoryHealth::healthy();
        assert!(health.is_healthy());
        
        let health = RepositoryHealth::unhealthy("Connection failed".to_string());
        assert!(!health.is_healthy());
    }
    
    #[test]
    fn test_contextual_error() {
        let error = OrganizationError::organization_not_found("test-org");
        let contextual = error.with_context("get_organization", "organization_service");
        
        assert_eq!(contextual.context.operation, "get_organization");
        assert_eq!(contextual.context.component, "organization_service");
    }
}