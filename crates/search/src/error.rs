//! Error types for Search bounded context
//!
//! Defines all error types used within the search domain
//! Following robust error handling patterns with thiserror

use thiserror::Error;

/// Main error type for the Search bounded context
#[derive(Error, Debug)]
pub enum SearchError {
    #[error("Search query failed: {message}")]
    QueryFailed { message: String },

    #[error("Search index not found: {index_name}")]
    IndexNotFound { index_name: String },

    #[error("Invalid search query: {reason}")]
    InvalidQuery { reason: String },

    #[error("Search timeout after {timeout_ms}ms")]
    SearchTimeout { timeout_ms: u64 },

    #[error("Index operation failed: {operation} - {reason}")]
    IndexOperationFailed { operation: String, reason: String },

    #[error("Search service unavailable: {reason}")]
    ServiceUnavailable { reason: String },

    #[error("Invalid search parameters: {field} - {reason}")]
    InvalidParameters { field: String, reason: String },

    #[error("Search result parsing failed: {reason}")]
    ResultParsingFailed { reason: String },

    #[error("Index corruption detected: {index_name} - {details}")]
    IndexCorruption { index_name: String, details: String },

    #[error("Search rate limit exceeded for user: {user_id}")]
    RateLimitExceeded { user_id: String },

    #[error("Infrastructure error: {source}")]
    Infrastructure {
        #[from]
        source: anyhow::Error,
    },

    #[error("Database error: {message}")]
    Database { message: String },

    #[error("Network error: {message}")]
    Network { message: String },

    #[error("Authentication error: {message}")]
    Authentication { message: String },

    #[error("Authorization error: {message}")]
    Authorization { message: String },
}

impl SearchError {
    /// Create a new query failed error
    pub fn query_failed(message: impl Into<String>) -> Self {
        Self::QueryFailed {
            message: message.into(),
        }
    }

    /// Create a new index not found error
    pub fn index_not_found(index_name: impl Into<String>) -> Self {
        Self::IndexNotFound {
            index_name: index_name.into(),
        }
    }

    /// Create a new invalid query error
    pub fn invalid_query(reason: impl Into<String>) -> Self {
        Self::InvalidQuery {
            reason: reason.into(),
        }
    }

    /// Create a new search timeout error
    pub fn search_timeout(timeout_ms: u64) -> Self {
        Self::SearchTimeout { timeout_ms }
    }

    /// Create a new index operation failed error
    pub fn index_operation_failed(
        operation: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self::IndexOperationFailed {
            operation: operation.into(),
            reason: reason.into(),
        }
    }

    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SearchError::SearchTimeout { .. }
                | SearchError::ServiceUnavailable { .. }
                | SearchError::Network { .. }
                | SearchError::Infrastructure { .. }
        )
    }

    /// Get the error category for metrics and logging
    pub fn category(&self) -> &'static str {
        match self {
            SearchError::QueryFailed { .. } => "query_error",
            SearchError::IndexNotFound { .. } => "index_error",
            SearchError::InvalidQuery { .. } => "validation_error",
            SearchError::SearchTimeout { .. } => "timeout_error",
            SearchError::IndexOperationFailed { .. } => "index_error",
            SearchError::ServiceUnavailable { .. } => "service_error",
            SearchError::InvalidParameters { .. } => "validation_error",
            SearchError::ResultParsingFailed { .. } => "parsing_error",
            SearchError::IndexCorruption { .. } => "corruption_error",
            SearchError::RateLimitExceeded { .. } => "rate_limit_error",
            SearchError::Infrastructure { .. } => "infrastructure_error",
            SearchError::Database { .. } => "database_error",
            SearchError::Network { .. } => "network_error",
            SearchError::Authentication { .. } => "auth_error",
            SearchError::Authorization { .. } => "authz_error",
        }
    }
}

/// Result type alias for Search operations
pub type SearchResult<T> = Result<T, SearchError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let error = SearchError::query_failed("syntax error");
        assert!(matches!(error, SearchError::QueryFailed { .. }));
    }

    #[test]
    fn test_error_categories() {
        let error = SearchError::search_timeout(5000);
        assert_eq!(error.category(), "timeout_error");
        assert!(error.is_retryable());
    }
}
