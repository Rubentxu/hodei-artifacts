//! Search error types and handling
//!
//! This module defines all error types used throughout the search bounded context,
//! following the error handling patterns established in the project.

use thiserror::Error;

/// Search crate error types
#[derive(Debug, Error)]
pub enum SearchError {
    #[error("Query parsing error: {0}")]
    QueryParse(String),
    
    #[error("Index not found: {0}")]
    IndexNotFound(String),
    
    #[error("Document not found: {0}")]
    DocumentNotFound(String),
    
    #[error("Index operation failed: {0}")]
    IndexOperationFailed(String),
    
    #[error("Search execution failed: {0}")]
    SearchExecutionFailed(String),
    
    #[error("Invalid search query: {0}")]
    InvalidQuery(String),
    
    #[error("Invalid filter: {0}")]
    InvalidFilter(String),
    
    #[error("Invalid pagination: {0}")]
    InvalidPagination(String),
    
    #[error("Timeout exceeded: {0}ms")]
    Timeout(u64),
    
    #[error("Configuration error: {0}")]
    Configuration(String),
    
    #[error("Storage error: {0}")]
    Storage(String),
    
    #[error("Network error: {0}")]
    Network(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Resource busy: {0}")]
    ResourceBusy(String),
    
    #[error("Corrupted index: {0}")]
    CorruptedIndex(String),
    
    #[error("Feature not supported: {0}")]
    UnsupportedFeature(String),
    
    #[error("Validation error: {0}")]
    Validation(String),
    
    #[error("Internal error: {0}")]
    Internal(String),
}

impl SearchError {
    /// Create a query parsing error
    pub fn query_parse<S: Into<String>>(message: S) -> Self {
        Self::QueryParse(message.into())
    }
    
    /// Create an index not found error
    pub fn index_not_found<S: Into<String>>(index_id: S) -> Self {
        Self::IndexNotFound(index_id.into())
    }
    
    /// Create a document not found error
    pub fn document_not_found<S: Into<String>>(doc_id: S) -> Self {
        Self::DocumentNotFound(doc_id.into())
    }
    
    /// Create an index operation failed error
    pub fn index_operation_failed<S: Into<String>>(message: S) -> Self {
        Self::IndexOperationFailed(message.into())
    }
    
    /// Create a search execution failed error
    pub fn search_execution_failed<S: Into<String>>(message: S) -> Self {
        Self::SearchExecutionFailed(message.into())
    }
    
    /// Create an invalid query error
    pub fn invalid_query<S: Into<String>>(message: S) -> Self {
        Self::InvalidQuery(message.into())
    }
    
    /// Create a timeout error
    pub fn timeout(timeout_ms: u64) -> Self {
        Self::Timeout(timeout_ms)
    }
    
    /// Create a configuration error
    pub fn configuration<S: Into<String>>(message: S) -> Self {
        Self::Configuration(message.into())
    }
    
    /// Create a validation error
    pub fn validation<S: Into<String>>(message: S) -> Self {
        Self::Validation(message.into())
    }
    
    /// Create an internal error
    pub fn internal<S: Into<String>>(message: S) -> Self {
        Self::Internal(message.into())
    }
    
    /// Check if this error is retryable
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            SearchError::ResourceBusy(_) 
                | SearchError::Network(_) 
                | SearchError::Timeout(_)
        )
    }
    
    /// Check if this error indicates a corrupted state
    pub fn is_corruption_error(&self) -> bool {
        matches!(self, SearchError::CorruptedIndex(_))
    }
    
    /// Get the error category
    pub fn category(&self) -> ErrorCategory {
        match self {
            SearchError::QueryParse(_) => ErrorCategory::Query,
            SearchError::IndexNotFound(_) => ErrorCategory::NotFound,
            SearchError::DocumentNotFound(_) => ErrorCategory::NotFound,
            SearchError::IndexOperationFailed(_) => ErrorCategory::Operation,
            SearchError::SearchExecutionFailed(_) => ErrorCategory::Operation,
            SearchError::InvalidQuery(_) => ErrorCategory::Validation,
            SearchError::InvalidFilter(_) => ErrorCategory::Validation,
            SearchError::InvalidPagination(_) => ErrorCategory::Validation,
            SearchError::Timeout(_) => ErrorCategory::Timeout,
            SearchError::Configuration(_) => ErrorCategory::Configuration,
            SearchError::Storage(_) => ErrorCategory::Storage,
            SearchError::Network(_) => ErrorCategory::Network,
            SearchError::PermissionDenied(_) => ErrorCategory::Permission,
            SearchError::ResourceBusy(_) => ErrorCategory::Resource,
            SearchError::CorruptedIndex(_) => ErrorCategory::Corruption,
            SearchError::UnsupportedFeature(_) => ErrorCategory::Unsupported,
            SearchError::Validation(_) => ErrorCategory::Validation,
            SearchError::Internal(_) => ErrorCategory::Internal,
        }
    }
    
    /// Get a user-friendly error message
    pub fn user_message(&self) -> String {
        match self {
            SearchError::QueryParse(msg) => format!("The search query could not be understood: {}", msg),
            SearchError::IndexNotFound(id) => format!("Search index '{}' does not exist", id),
            SearchError::DocumentNotFound(id) => format!("Document '{}' could not be found", id),
            SearchError::IndexOperationFailed(msg) => format!("Failed to perform index operation: {}", msg),
            SearchError::SearchExecutionFailed(msg) => format!("Search failed: {}", msg),
            SearchError::InvalidQuery(msg) => format!("Invalid search query: {}", msg),
            SearchError::InvalidFilter(msg) => format!("Invalid search filter: {}", msg),
            SearchError::InvalidPagination(msg) => format!("Invalid pagination settings: {}", msg),
            SearchError::Timeout(ms) => format!("Search timed out after {}ms", ms),
            SearchError::Configuration(msg) => format!("Configuration error: {}", msg),
            SearchError::Storage(msg) => format!("Storage error: {}", msg),
            SearchError::Network(msg) => format!("Network error: {}", msg),
            SearchError::PermissionDenied(msg) => format!("Permission denied: {}", msg),
            SearchError::ResourceBusy(msg) => format!("Resource is busy: {}", msg),
            SearchError::CorruptedIndex(msg) => format!("Search index is corrupted: {}", msg),
            SearchError::UnsupportedFeature(msg) => format!("Feature not supported: {}", msg),
            SearchError::Validation(msg) => format!("Validation error: {}", msg),
            SearchError::Internal(msg) => format!("Internal error occurred: {}", msg),
        }
    }
}

/// Error categories for better error handling
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorCategory {
    Query,
    NotFound,
    Operation,
    Validation,
    Timeout,
    Configuration,
    Storage,
    Network,
    Permission,
    Resource,
    Corruption,
    Unsupported,
    Internal,
}

/// Search result type alias for better error handling
pub type SearchResult<T> = Result<T, SearchError>;

/// Query builder for constructing search queries with validation
pub struct QueryBuilder {
    query: Option<String>,
    index_ids: Vec<crate::domain::IndexId>,
    filters: crate::domain::query::QueryFilters,
    pagination: crate::domain::query::Pagination,
    options: crate::domain::query::SearchOptions,
}

impl QueryBuilder {
    pub fn new() -> Self {
        Self {
            query: None,
            index_ids: Vec::new(),
            filters: crate::domain::query::QueryFilters::default(),
            pagination: crate::domain::query::Pagination::default(),
            options: crate::domain::query::SearchOptions::default(),
        }
    }
    
    pub fn query<S: Into<String>>(mut self, query: S) -> Self {
        self.query = Some(query.into());
        self
    }
    
    pub fn index_id(mut self, index_id: crate::domain::IndexId) -> Self {
        self.index_ids.push(index_id);
        self
    }
    
    pub fn index_ids(mut self, index_ids: Vec<crate::domain::IndexId>) -> Self {
        self.index_ids = index_ids;
        self
    }
    
    pub fn filters(mut self, filters: crate::domain::query::QueryFilters) -> Self {
        self.filters = filters;
        self
    }
    
    pub fn pagination(mut self, pagination: crate::domain::query::Pagination) -> Self {
        self.pagination = pagination;
        self
    }
    
    pub fn options(mut self, options: crate::domain::query::SearchOptions) -> Self {
        self.options = options;
        self
    }
    
    pub fn build(self) -> SearchResult<crate::domain::query::SearchQuery> {
        let query = self.query.ok_or_else(|| SearchError::validation("Query text is required"))?;
        
        if query.trim().is_empty() {
            return Err(SearchError::validation("Query text cannot be empty"));
        }
        
        if !self.pagination.is_valid() {
            return Err(SearchError::validation("Invalid pagination parameters"));
        }
        
        Ok(crate::domain::query::SearchQuery::new(query)
            .with_indices(self.index_ids)
            .with_filters(self.filters)
            .with_pagination(self.pagination)
            .with_options(self.options))
    }
}

impl Default for QueryBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Conversion from other error types
impl From<shared::models::ApiError> for SearchError {
    fn from(api_error: shared::models::ApiError) -> Self {
        match api_error.status_code {
            404 => SearchError::not_found("Resource not found"),
            403 => SearchError::permission_denied("Access denied"),
            400 => SearchError::validation("Invalid request"),
            408 => SearchError::timeout(30000), // Default timeout
            500 => SearchError::internal("Internal server error"),
            503 => SearchError::resource_busy("Service unavailable"),
            _ => SearchError::internal(api_error.message),
        }
    }
}

/// Helper methods for common error creation
impl SearchError {
    pub fn not_found<S: Into<String>>(message: S) -> Self {
        Self::DocumentNotFound(message.into())
    }
    
    pub fn permission_denied<S: Into<String>>(message: S) -> Self {
        Self::PermissionDenied(message.into())
    }
    
    pub fn resource_busy<S: Into<String>>(message: S) -> Self {
        Self::ResourceBusy(message.into())
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
    pub fn new<O: Into<String>, C: Into<String>>(operation: O, component: C) -> Self {
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
    pub error: SearchError,
    pub context: ErrorContext,
}

impl ContextualError {
    pub fn new(error: SearchError, context: ErrorContext) -> Self {
        Self { error, context }
    }
    
    pub fn context<S: Into<String>>(self, operation: S, component: S) -> Self {
        Self {
            context: ErrorContext::new(operation, component),
            ..self
        }
    }
    
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.context.metadata.insert(key, value);
        self
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

impl From<SearchError> for ContextualError {
    fn from(error: SearchError) -> Self {
        Self {
            error,
            context: ErrorContext::new("unknown", "unknown"),
        }
    }
}

/// Result type with contextual error
pub type ContextualResult<T> = Result<T, ContextualError>;

/// Extension trait for adding context to results
pub trait ResultExt<T> {
    fn with_context<C: Into<String>, O: Into<String>>(
        self,
        operation: O,
        component: C,
    ) -> ContextualResult<T>;
    
    fn with_metadata(self, key: String, value: String) -> ContextualResult<T>;
}

impl<T> ResultExt<T> for SearchResult<T> {
    fn with_context<C: Into<String>, O: Into<String>>(
        self,
        operation: O,
        component: C,
    ) -> ContextualResult<T> {
        self.map_err(|error| ContextualError::new(
            error,
            ErrorContext::new(operation, component),
        ))
    }
    
    fn with_metadata(self, key: String, value: String) -> ContextualResult<T> {
        self.map_err(|error| ContextualError::from(error).with_metadata(key, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_error_categories() {
        assert_eq!(SearchError::query_parse("test").category(), ErrorCategory::Query);
        assert_eq!(SearchError::index_not_found("test").category(), ErrorCategory::NotFound);
        assert_eq!(SearchError::timeout(1000).category(), ErrorCategory::Timeout);
    }
    
    #[test]
    fn test_retryable_errors() {
        assert!(SearchError::timeout(1000).is_retryable());
        assert!(SearchError::network("test").is_retryable());
        assert!(!SearchError::validation("test").is_retryable());
    }
    
    #[test]
    fn test_user_messages() {
        let msg = SearchError::index_not_found("test").user_message();
        assert!(msg.contains("test"));
        assert!(msg.contains("does not exist"));
    }
    
    #[test]
    fn test_query_builder() {
        let result = QueryBuilder::new()
            .query("test")
            .build();
        
        assert!(result.is_ok());
        
        let query = result.unwrap();
        assert_eq!(query.query, "test");
    }
    
    #[test]
    fn test_query_builder_validation() {
        let result = QueryBuilder::new().build();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SearchError::Validation(_)));
    }
}