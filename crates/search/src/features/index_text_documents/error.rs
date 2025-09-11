//! Error types for Index Text Documents Feature
//!
//! This module defines comprehensive error types following the error handling patterns
//! established in the project. Each error is specific and provides meaningful context.

use thiserror::Error;

/// Comprehensive error type for the Index Text Documents feature
#[derive(Debug, Error)]
pub enum IndexDocumentError {
    /// Errors related to document indexing operations
    #[error("Indexing error: {source}")]
    Indexing {
        #[from]
        source: IndexError,
    },
    
    /// Errors related to text analysis operations
    #[error("Text analysis error: {source}")]
    TextAnalysis {
        #[from]
        source: AnalysisError,
    },
    
    /// Errors related to tokenization
    #[error("Tokenization error: {source}")]
    Tokenization {
        #[from]
        source: TokenizationError,
    },
    
    /// Errors related to stemming
    #[error("Stemming error: {source}")]
    Stemming {
        #[from]
        source: StemmingError,
    },
    
    /// Errors related to stop word removal
    #[error("Stop word removal error: {source}")]
    StopWordRemoval {
        #[from]
        source: StopWordError,
    },
    
    /// Errors related to language detection
    #[error("Language detection error: {source}")]
    LanguageDetection {
        #[from]
        source: LanguageDetectionError,
    },
    
    /// Errors related to index health monitoring
    #[error("Health monitoring error: {source}")]
    HealthMonitoring {
        #[from]
        source: HealthError,
    },
    
    /// Errors related to statistics
    #[error("Statistics error: {source}")]
    Statistics {
        #[from]
        source: StatsError,
    },
    
    /// Errors related to metrics
    #[error("Metrics error: {source}")]
    Metrics {
        #[from]
        source: MetricsError,
    },
    
    /// Errors related to memory monitoring
    #[error("Memory monitoring error: {source}")]
    MemoryMonitoring {
        #[from]
        source: MemoryError,
    },
    
    /// Errors related to optimization
    #[error("Optimization error: {source}")]
    Optimization {
        #[from]
        source: OptimizationError,
    },
    
    /// Errors related to schema management
    #[error("Schema management error: {source}")]
    SchemaManagement {
        #[from]
        source: SchemaError,
    },
    
    /// Errors related to document validation
    #[error("Document validation error: {source}")]
    DocumentValidation {
        #[from]
        source: ValidationError,
    },
    
    /// Errors related to duplicate detection
    #[error("Duplicate detection error: {source}")]
    DuplicateDetection {
        #[from]
        source: DuplicateError,
    },
    
    /// Configuration errors
    #[error("Configuration error: {0}")]
    Configuration(String),
    
    /// Timeout errors
    #[error("Operation timeout: {0}ms")]
    Timeout(u64),
    
    /// Resource unavailable errors
    #[error("Resource unavailable: {0}")]
    ResourceUnavailable(String),
    
    /// Permission errors
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    /// Quota exceeded errors
    #[error("Quota exceeded: {0}")]
    QuotaExceeded(String),
    
    /// Rate limiting errors
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),
    
    /// Validation errors for business rules
    #[error("Business rule validation failed: {0}")]
    BusinessRuleValidation(String),
    
    /// Concurrency errors
    #[error("Concurrency error: {0}")]
    Concurrency(String),
    
    /// Serialization/deserialization errors
    #[error("Serialization error: {0}")]
    Serialization(String),
    
    /// Network errors
    #[error("Network error: {0}")]
    Network(String),
    
    /// Database errors
    #[error("Database error: {0}")]
    Database(String),
    
    /// Storage errors
    #[error("Storage error: {0}")]
    Storage(String),
    
    /// External service errors
    #[error("External service error: {service}: {error}")]
    ExternalService {
        service: String,
        error: String,
    },
    
    /// Unexpected errors
    #[error("Unexpected error: {0}")]
    Unexpected(String),
}

/// Re-export the specific error types from the ports module for convenience
pub use ports::{
    IndexError,
    AnalysisError,
    TokenizationError,
    StemmingError,
    StopWordError,
    LanguageDetectionError,
    HealthError,
    StatsError,
    MetricsError,
    MemoryError,
    OptimizationError,
    SchemaError,
    ValidationError,
    DuplicateError,
};

/// Result type for the Index Text Documents feature
pub type IndexDocumentResult<T> = Result<T, IndexDocumentError>;

/// Helper functions for error handling
impl IndexDocumentError {
    /// Create a configuration error
    pub fn configuration<S: Into<String>>(message: S) -> Self {
        IndexDocumentError::Configuration(message.into())
    }
    
    /// Create a timeout error
    pub fn timeout(duration_ms: u64) -> Self {
        IndexDocumentError::Timeout(duration_ms)
    }
    
    /// Create a resource unavailable error
    pub fn resource_unavailable<S: Into<String>>(resource: S) -> Self {
        IndexDocumentError::ResourceUnavailable(resource.into())
    }
    
    /// Create a permission denied error
    pub fn permission_denied<S: Into<String>>(operation: S) -> Self {
        IndexDocumentError::PermissionDenied(operation.into())
    }
    
    /// Create a quota exceeded error
    pub fn quota_exceeded<S: Into<String>>(quota_type: S) -> Self {
        IndexDocumentError::QuotaExceeded(quota_type.into())
    }
    
    /// Create a rate limit exceeded error
    pub fn rate_limit_exceeded<S: Into<String>>(limit_type: S) -> Self {
        IndexDocumentError::RateLimitExceeded(limit_type.into())
    }
    
    /// Create a business rule validation error
    pub fn business_rule_validation<S: Into<String>>(rule: S) -> Self {
        IndexDocumentError::BusinessRuleValidation(rule.into())
    }
    
    /// Create a concurrency error
    pub fn concurrency<S: Into<String>>(message: S) -> Self {
        IndexDocumentError::Concurrency(message.into())
    }
    
    /// Create a serialization error
    pub fn serialization<S: Into<String>>(message: S) -> Self {
        IndexDocumentError::Serialization(message.into())
    }
    
    /// Create a network error
    pub fn network<S: Into<String>>(message: S) -> Self {
        IndexDocumentError::Network(message.into())
    }
    
    /// Create a database error
    pub fn database<S: Into<String>>(message: S) -> Self {
        IndexDocumentError::Database(message.into())
    }
    
    /// Create a storage error
    pub fn storage<S: Into<String>>(message: S) -> Self {
        IndexDocumentError::Storage(message.into())
    }
    
    /// Create an external service error
    pub fn external_service<S: Into<String>, E: Into<String>>(service: S, error: E) -> Self {
        IndexDocumentError::ExternalService {
            service: service.into(),
            error: error.into(),
        }
    }
    
    /// Create an unexpected error
    pub fn unexpected<S: Into<String>>(message: S) -> Self {
        IndexDocumentError::Unexpected(message.into())
    }
}

/// Helper trait for converting external errors to IndexDocumentError
pub trait ToIndexDocumentError<T> {
    fn to_index_document_error(self) -> IndexDocumentResult<T>;
}

/// Implementation for common external error types
impl<T> ToIndexDocumentError<T> for Result<T, mongodb::error::Error> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::database(format!("MongoDB error: {}", e)))
    }
}

impl<T> ToIndexDocumentError<T> for Result<T, tantivy::TantivyError> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::external_service("Tantivy", e.to_string()))
    }
}

impl<T> ToIndexDocumentError<T> for Result<T, serde_json::Error> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::serialization(format!("JSON error: {}", e)))
    }
}

impl<T> ToIndexDocumentError<T> for Result<T, std::io::Error> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::storage(format!("IO error: {}", e)))
    }
}

impl<T> ToIndexDocumentError<T> for Result<T, ValidationError> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::DocumentValidation { source: e })
    }
}

impl<T> ToIndexDocumentError<T> for Result<T, DuplicateError> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::DuplicateDetection { source: e })
    }
}

impl<T> ToIndexDocumentError<T> for Result<T, HealthError> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::HealthMonitoring { source: e })
    }
}

impl<T> ToIndexDocumentError<T> for Result<T, AnalysisError> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::TextAnalysis { source: e })
    }
}

impl<T> ToIndexDocumentError<T> for Result<T, IndexError> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::Indexing { source: e })
    }
}

impl<T> ToIndexDocumentError<T> for Result<T, StatsError> {
    fn to_index_document_error(self) -> IndexDocumentResult<T> {
        self.map_err(|e| IndexDocumentError::Statistics { source: e })
    }
}

/// Error context information for better debugging
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Operation that failed
    pub operation: String,
    /// Document or resource identifier
    pub resource_id: Option<String>,
    /// Additional context data
    pub context: std::collections::HashMap<String, String>,
    /// Timestamp of the error
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new<S: Into<String>>(operation: S) -> Self {
        Self {
            operation: operation.into(),
            resource_id: None,
            context: std::collections::HashMap::new(),
            timestamp: chrono::Utc::now(),
        }
    }
    
    /// Add a resource identifier
    pub fn with_resource_id<S: Into<String>>(mut self, resource_id: S) -> Self {
        self.resource_id = Some(resource_id.into());
        self
    }
    
    /// Add context data
    pub fn with_context<S: Into<String>, V: Into<String>>(mut self, key: S, value: V) -> Self {
        self.context.insert(key.into(), value.into());
        self
    }
    
    /// Add multiple context data from a hash map
    pub fn with_context_map(mut self, context: std::collections::HashMap<String, String>) -> Self {
        self.context.extend(context);
        self
    }
}

/// Enhanced error with context
#[derive(Debug, Error)]
pub struct ContextualizedError {
    /// The original error
    #[source]
    pub source: IndexDocumentError,
    /// Error context
    pub context: ErrorContext,
}

impl std::fmt::Display for ContextualizedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Error in operation '{}' (resource: {:?}): {}",
            self.context.operation,
            self.context.resource_id,
            self.source
        )
    }
}

/// Trait for adding context to errors
pub trait WithContext<T> {
    fn with_context(self, context: ErrorContext) -> Result<T, ContextualizedError>;
    
    fn with_operation_context<S: Into<String>>(self, operation: S) -> Result<T, ContextualizedError>;
    
    fn with_resource_context<S: Into<String>, R: Into<String>>(self, operation: S, resource_id: R) -> Result<T, ContextualizedError>;
}

impl<T> WithContext<T> for Result<T, IndexDocumentError> {
    fn with_context(self, context: ErrorContext) -> Result<T, ContextualizedError> {
        self.map_err(|source| ContextualizedError { source, context })
    }
    
    fn with_operation_context<S: Into<String>>(self, operation: S) -> Result<T, ContextualizedError> {
        self.with_context(ErrorContext::new(operation))
    }
    
    fn with_resource_context<S: Into<String>, R: Into<String>>(self, operation: S, resource_id: R) -> Result<T, ContextualizedError> {
        self.with_context(ErrorContext::new(operation).with_resource_id(resource_id))
    }
}

/// Error severity levels
#[derive(Debug, Clone, PartialEq, PartialOrd)]
pub enum ErrorSeverity {
    /// Informational message
    Info,
    /// Warning that doesn't stop operation
    Warning,
    /// Error that affects the current operation
    Error,
    /// Critical error that requires immediate attention
    Critical,
}

/// Error categorization for monitoring and alerting
#[derive(Debug, Clone)]
pub struct ErrorCategory {
    /// Category name
    pub name: String,
    /// Severity level
    pub severity: ErrorSeverity,
    /// Whether this error should trigger an alert
    pub should_alert: bool,
    /// Error type for grouping
    pub error_type: String,
    /// Component where the error occurred
    pub component: String,
}

impl IndexDocumentError {
    /// Get the category for this error
    pub fn category(&self) -> ErrorCategory {
        match self {
            // Configuration errors
            IndexDocumentError::Configuration(_) => ErrorCategory {
                name: "Configuration".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "configuration_error".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Timeout errors
            IndexDocumentError::Timeout(_) => ErrorCategory {
                name: "Timeout".to_string(),
                severity: ErrorSeverity::Warning,
                should_alert: false,
                error_type: "timeout_error".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Resource unavailable errors
            IndexDocumentError::ResourceUnavailable(_) => ErrorCategory {
                name: "Resource Unavailable".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "resource_unavailable".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Indexing errors
            IndexDocumentError::Indexing { .. } => ErrorCategory {
                name: "Indexing".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "indexing_error".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Business rule validation errors
            IndexDocumentError::BusinessRuleValidation(_) => ErrorCategory {
                name: "Business Rule".to_string(),
                severity: ErrorSeverity::Warning,
                should_alert: false,
                error_type: "business_rule_violation".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Rate limiting errors
            IndexDocumentError::RateLimitExceeded(_) => ErrorCategory {
                name: "Rate Limit".to_string(),
                severity: ErrorSeverity::Warning,
                should_alert: false,
                error_type: "rate_limit_exceeded".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Concurrency errors
            IndexDocumentError::Concurrency(_) => ErrorCategory {
                name: "Concurrency".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "concurrency_error".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // External service errors
            IndexDocumentError::ExternalService { .. } => ErrorCategory {
                name: "External Service".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "external_service_error".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Database errors
            IndexDocumentError::Database(_) => ErrorCategory {
                name: "Database".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "database_error".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Storage errors
            IndexDocumentError::Storage(_) => ErrorCategory {
                name: "Storage".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "storage_error".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Unexpected errors
            IndexDocumentError::Unexpected(_) => ErrorCategory {
                name: "Unexpected".to_string(),
                severity: ErrorSeverity::Critical,
                should_alert: true,
                error_type: "unexpected_error".to_string(),
                component: "index_text_documents".to_string(),
            },
            
            // Other errors
            _ => ErrorCategory {
                name: "General".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: false,
                error_type: "general_error".to_string(),
                component: "index_text_documents".to_string(),
            },
        }
    }
    
    /// Check if this error should be retried
    pub fn should_retry(&self) -> bool {
        match self {
            IndexDocumentError::Timeout(_) => true,
            IndexDocumentError::ResourceUnavailable(_) => true,
            IndexDocumentError::RateLimitExceeded(_) => true,
            IndexDocumentError::Network(_) => true,
            IndexDocumentError::ExternalService { .. } => true,
            IndexDocumentError::Concurrency(_) => true,
            _ => false,
        }
    }
    
    /// Get suggested retry delay in milliseconds
    pub fn retry_delay_ms(&self) -> Option<u64> {
        match self {
            IndexDocumentError::RateLimitExceeded(_) => Some(1000), // 1 second
            IndexDocumentError::Timeout(_) => Some(500), // 500ms
            IndexDocumentError::ResourceUnavailable(_) => Some(2000), // 2 seconds
            IndexDocumentError::Network(_) => Some(1000), // 1 second
            IndexDocumentError::ExternalService { .. } => Some(1000), // 1 second
            IndexDocumentError::Concurrency(_) => Some(100), // 100ms
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_category_severity() {
        let config_error = IndexDocumentError::configuration("test config");
        let category = config_error.category();
        assert_eq!(category.severity, ErrorSeverity::Error);
        assert!(category.should_alert);
    }

    #[test]
    fn test_retry_logic() {
        assert!(IndexDocumentError::timeout(1000).should_retry());
        assert!(!IndexDocumentError::configuration("test").should_retry());
    }

    #[test]
    fn test_retry_delay() {
        assert_eq!(IndexDocumentError::rate_limit_exceeded("test").retry_delay_ms(), Some(1000));
        assert_eq!(IndexDocumentError::timeout(1000).retry_delay_ms(), Some(500));
        assert_eq!(IndexDocumentError::configuration("test").retry_delay_ms(), None);
    }

    #[test]
    fn test_error_context() {
        let context = ErrorContext::new("test_operation")
            .with_resource_id("doc123")
            .with_context("key1", "value1");
        
        assert_eq!(context.operation, "test_operation");
        assert_eq!(context.resource_id, Some("doc123".to_string()));
        assert_eq!(context.context.get("key1"), Some(&"value1".to_string()));
    }

    #[test]
    fn test_contextualized_error() {
        let result: Result<(), IndexDocumentError> = Err(IndexDocumentError::configuration("test"));
        let contextualized = result.with_operation_context("test_operation");
        
        assert!(contextualized.is_err());
        if let Err(err) = contextualized {
            assert_eq!(err.context.operation, "test_operation");
        }
    }
}