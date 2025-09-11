//! Error types for Full Text Search Feature
//!
//! This module defines comprehensive error types following the error handling patterns
//! established in the project. Each error is specific and provides meaningful context.

use thiserror::Error;
use ports::ConfigError;

/// Comprehensive error type for the Full Text Search feature
#[derive(Debug, Error)]
pub enum FullTextSearchError {
    /// Errors related to search operations
    #[error("Search error: {source}")]
    Search {
        #[from]
        source: SearchError,
    },
    
    /// Errors related to query analysis
    #[error("Query analysis error: {source}")]
    QueryAnalysis {
        #[from]
        source: AnalysisError,
    },
    
    /// Errors related to query parsing
    #[error("Query parsing error: {source}")]
    QueryParsing {
        #[from]
        source: ParseError,
    },
    
    /// Errors related to query optimization
    #[error("Query optimization error: {source}")]
    QueryOptimization {
        #[from]
        source: OptimizationError,
    },
    
    /// Errors related to term extraction
    #[error("Term extraction error: {source}")]
    TermExtraction {
        #[from]
        source: ExtractionError,
    },
    
    /// Errors related to query rewriting
    #[error("Query rewriting error: {source}")]
    QueryRewriting {
        #[from]
        source: RewriteError,
    },
    
    /// Errors related to relevance scoring
    #[error("Relevance scoring error: {source}")]
    RelevanceScoring {
        #[from]
        source: ScoreError,
    },
    
    /// Errors related to document ranking
    #[error("Document ranking error: {source}")]
    DocumentRanking {
        #[from]
        source: RankingError,
    },
    
    /// Errors related to result highlighting
    #[error("Result highlighting error: {source}")]
    ResultHighlighting {
        #[from]
        source: HighlightError,
    },
    
    /// Errors related to snippet generation
    #[error("Snippet generation error: {source}")]
    SnippetGeneration {
        #[from]
        source: SnippetError,
    },
    
    /// Errors related to passage extraction
    #[error("Passage extraction error: {source}")]
    PassageExtraction {
        #[from]
        source: PassageError,
    },
    
    /// Errors related to suggestions
    #[error("Search suggestions error: {source}")]
    Suggestions {
        #[from]
        source: SuggestionError,
    },
    
    /// Errors related to facets
    #[error("Facets error: {source}")]
    Facets {
        #[from]
        source: FacetError,
    },
    
    /// Errors related to performance monitoring
    #[error("Performance monitoring error: {source}")]
    PerformanceMonitoring {
        #[from]
        source: MonitoringError,
    },
    
    /// Errors related to statistics
    #[error("Statistics error: {source}")]
    Statistics {
        #[from]
        source: StatsError,
    },
    
    /// Errors related to queries
    #[error("Query error: {source}")]
    Query {
        #[from]
        source: QueryError,
    },
    
    /// Errors related to health monitoring
    #[error("Health monitoring error: {source}")]
    HealthMonitoring {
        #[from]
        source: HealthError,
    },
    
    /// Errors related to pattern analysis
    #[error("Pattern analysis error: {source}")]
    PatternAnalysis {
        #[from]
        source: PatternError,
    },
    
    /// Errors related to index operations
    #[error("Index operations error: {source}")]
    IndexOperations {
        #[from]
        source: IndexError,
    },
    
    /// Errors related to configuration
    #[error("Configuration error: {source}")]
    Configuration {
        #[from]
        source: ConfigError,
    },
    
    /// Errors related to maintenance
    #[error("Maintenance error: {source}")]
    Maintenance {
        #[from]
        source: MaintenanceError,
    },
    
    /// Errors related to segments
    #[error("Segment error: {source}")]
    Segment {
        #[from]
        source: SegmentError,
    },
    
    /// Errors related to merging
    #[error("Merge error: {source}")]
    Merge {
        #[from]
        source: MergeError,
    },
    
    /// Configuration errors
    #[error("Configuration error: {0}")]
    InvalidConfiguration(String),
    
    /// Timeout errors
    #[error("Operation timeout: {0}ms")]
    Timeout(u64),
    
    /// Resource unavailable errors
    #[error("Resource unavailable: {0}")]
    ResourceUnavailable(String),
    
    /// Permission errors
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
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
    SearchError,
    SuggestionError,
    FacetError,
    AnalysisError,
    ParseError,
    OptimizationError,
    ExtractionError,
    RewriteError,
    ScoreError,
    RankingError,
    HighlightError,
    SnippetError,
    PassageError,
    MonitoringError,
    StatsError,
    QueryError,
    HealthError,
    PatternError,
    IndexError,
    // ConfigError is defined in ports.rs to avoid conflicts
    // MaintenanceError, SegmentError, MergeError are defined in error.rs
};

/// Result type for the Full Text Search feature
pub type FullTextSearchResult<T> = Result<T, FullTextSearchError>;

/// Helper functions for error handling
impl FullTextSearchError {
    /// Create a configuration error
    pub fn invalid_configuration<S: Into<String>>(message: S) -> Self {
        FullTextSearchError::InvalidConfiguration(message.into())
    }
    
    /// Create a timeout error
    pub fn timeout(duration_ms: u64) -> Self {
        FullTextSearchError::Timeout(duration_ms)
    }
    
    /// Create a resource unavailable error
    pub fn resource_unavailable<S: Into<String>>(resource: S) -> Self {
        FullTextSearchError::ResourceUnavailable(resource.into())
    }
    
    /// Create a permission denied error
    pub fn permission_denied<S: Into<String>>(operation: S) -> Self {
        FullTextSearchError::PermissionDenied(operation.into())
    }
    
    /// Create a rate limit exceeded error
    pub fn rate_limit_exceeded<S: Into<String>>(limit_type: S) -> Self {
        FullTextSearchError::RateLimitExceeded(limit_type.into())
    }
    
    /// Create a business rule validation error
    pub fn business_rule_validation<S: Into<String>>(rule: S) -> Self {
        FullTextSearchError::BusinessRuleValidation(rule.into())
    }
    
    /// Create a concurrency error
    pub fn concurrency<S: Into<String>>(message: S) -> Self {
        FullTextSearchError::Concurrency(message.into())
    }
    
    /// Create a serialization error
    pub fn serialization<S: Into<String>>(message: S) -> Self {
        FullTextSearchError::Serialization(message.into())
    }
    
    /// Create a network error
    pub fn network<S: Into<String>>(message: S) -> Self {
        FullTextSearchError::Network(message.into())
    }
    
    /// Create a database error
    pub fn database<S: Into<String>>(message: S) -> Self {
        FullTextSearchError::Database(message.into())
    }
    
    /// Create a storage error
    pub fn storage<S: Into<String>>(message: S) -> Self {
        FullTextSearchError::Storage(message.into())
    }
    
    /// Create an external service error
    pub fn external_service<S: Into<String>, E: Into<String>>(service: S, error: E) -> Self {
        FullTextSearchError::ExternalService {
            service: service.into(),
            error: error.into(),
        }
    }
    
    /// Create an unexpected error
    pub fn unexpected<S: Into<String>>(message: S) -> Self {
        FullTextSearchError::Unexpected(message.into())
    }
}

/// Helper trait for converting external errors to FullTextSearchError
pub trait ToFullTextSearchError<T> {
    fn to_full_text_search_error(self) -> FullTextSearchResult<T>;
}

/// Implementation for common external error types
impl<T> ToFullTextSearchError<T> for Result<T, mongodb::error::Error> {
    fn to_full_text_search_error(self) -> FullTextSearchResult<T> {
        self.map_err(|e| FullTextSearchError::database(format!("MongoDB error: {}", e)))
    }
}

impl<T> ToFullTextSearchError<T> for Result<T, tantivy::TantivyError> {
    fn to_full_text_search_error(self) -> FullTextSearchResult<T> {
        self.map_err(|e| FullTextSearchError::external_service("Tantivy", e.to_string()))
    }
}

impl<T> ToFullTextSearchError<T> for Result<T, serde_json::Error> {
    fn to_full_text_search_error(self) -> FullTextSearchResult<T> {
        self.map_err(|e| FullTextSearchError::serialization(format!("JSON error: {}", e)))
    }
}

impl<T> ToFullTextSearchError<T> for Result<T, std::io::Error> {
    fn to_full_text_search_error(self) -> FullTextSearchResult<T> {
        self.map_err(|e| FullTextSearchError::storage(format!("IO error: {}", e)))
    }
}

/// Error context information for better debugging
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Operation that failed
    pub operation: String,
    /// Query or resource identifier
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
    pub source: FullTextSearchError,
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

impl<T> WithContext<T> for Result<T, FullTextSearchError> {
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

impl FullTextSearchError {
    /// Get the category for this error
    pub fn category(&self) -> ErrorCategory {
        match self {
            // Configuration errors
            FullTextSearchError::InvalidConfiguration(_) => ErrorCategory {
                name: "Configuration".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "configuration_error".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Timeout errors
            FullTextSearchError::Timeout(_) => ErrorCategory {
                name: "Timeout".to_string(),
                severity: ErrorSeverity::Warning,
                should_alert: false,
                error_type: "timeout_error".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Resource unavailable errors
            FullTextSearchError::ResourceUnavailable(_) => ErrorCategory {
                name: "Resource Unavailable".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "resource_unavailable".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Search errors
            FullTextSearchError::Search { .. } => ErrorCategory {
                name: "Search".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "search_error".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Business rule validation errors
            FullTextSearchError::BusinessRuleValidation(_) => ErrorCategory {
                name: "Business Rule".to_string(),
                severity: ErrorSeverity::Warning,
                should_alert: false,
                error_type: "business_rule_violation".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Rate limiting errors
            FullTextSearchError::RateLimitExceeded(_) => ErrorCategory {
                name: "Rate Limit".to_string(),
                severity: ErrorSeverity::Warning,
                should_alert: false,
                error_type: "rate_limit_exceeded".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Concurrency errors
            FullTextSearchError::Concurrency(_) => ErrorCategory {
                name: "Concurrency".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "concurrency_error".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // External service errors
            FullTextSearchError::ExternalService { .. } => ErrorCategory {
                name: "External Service".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "external_service_error".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Database errors
            FullTextSearchError::Database(_) => ErrorCategory {
                name: "Database".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "database_error".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Storage errors
            FullTextSearchError::Storage(_) => ErrorCategory {
                name: "Storage".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: true,
                error_type: "storage_error".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Unexpected errors
            FullTextSearchError::Unexpected(_) => ErrorCategory {
                name: "Unexpected".to_string(),
                severity: ErrorSeverity::Critical,
                should_alert: true,
                error_type: "unexpected_error".to_string(),
                component: "search_full_text".to_string(),
            },
            
            // Other errors
            _ => ErrorCategory {
                name: "General".to_string(),
                severity: ErrorSeverity::Error,
                should_alert: false,
                error_type: "general_error".to_string(),
                component: "search_full_text".to_string(),
            },
        }
    }
    
    /// Check if this error should be retried
    pub fn should_retry(&self) -> bool {
        match self {
            FullTextSearchError::Timeout(_) => true,
            FullTextSearchError::ResourceUnavailable(_) => true,
            FullTextSearchError::RateLimitExceeded(_) => true,
            FullTextSearchError::Network(_) => true,
            FullTextSearchError::ExternalService { .. } => true,
            FullTextSearchError::Concurrency(_) => true,
            FullTextSearchError::Search { source } => match source {
                SearchError::IndexUnavailable(_) => true,
                SearchError::Timeout(_) => true,
                SearchError::ResourceLimitExceeded(_) => true,
                _ => false,
            },
            _ => false,
        }
    }
    
    /// Get suggested retry delay in milliseconds
    pub fn retry_delay_ms(&self) -> Option<u64> {
        match self {
            FullTextSearchError::RateLimitExceeded(_) => Some(1000), // 1 second
            FullTextSearchError::Timeout(_) => Some(500), // 500ms
            FullTextSearchError::ResourceUnavailable(_) => Some(2000), // 2 seconds
            FullTextSearchError::Network(_) => Some(1000), // 1 second
            FullTextSearchError::ExternalService { .. } => Some(1000), // 1 second
            FullTextSearchError::Concurrency(_) => Some(100), // 100ms
            FullTextSearchError::Search { source } => match source {
                SearchError::Timeout(_) => Some(500),
                SearchError::IndexUnavailable(_) => Some(2000),
                SearchError::ResourceLimitExceeded(_) => Some(1000),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_category_severity() {
        let config_error = FullTextSearchError::invalid_configuration("test config");
        let category = config_error.category();
        assert_eq!(category.severity, ErrorSeverity::Error);
        assert!(category.should_alert);
    }

    #[test]
    fn test_retry_logic() {
        assert!(FullTextSearchError::timeout(1000).should_retry());
        assert!(!FullTextSearchError::invalid_configuration("test").should_retry());
    }

    #[test]
    fn test_retry_delay() {
        assert_eq!(FullTextSearchError::rate_limit_exceeded("test").retry_delay_ms(), Some(1000));
        assert_eq!(FullTextSearchError::timeout(1000).retry_delay_ms(), Some(500));
        assert_eq!(FullTextSearchError::invalid_configuration("test").retry_delay_ms(), None);
    }

    #[test]
    fn test_error_context() {
        let context = ErrorContext::new("test_operation")
            .with_resource_id("query123")
            .with_context("key1", "value1");
        
        assert_eq!(context.operation, "test_operation");
        assert_eq!(context.resource_id, Some("query123".to_string()));
        assert_eq!(context.context.get("key1"), Some(&"value1".to_string()));
    }

    #[test]
    fn test_contextualized_error() {
        let result: Result<(), FullTextSearchError> = Err(FullTextSearchError::invalid_configuration("test"));
        let contextualized = result.with_operation_context("test_operation");
        
        assert!(contextualized.is_err());
        if let Err(err) = contextualized {
            assert_eq!(err.context.operation, "test_operation");
        }
    }
}

// Index operation result types - these are now defined in ports.rs to avoid conflicts
// Use the types from ports.rs instead

// Additional error types for index management
#[derive(Debug, thiserror::Error)]
pub enum RebuildError {
    #[error("Index rebuild failed: {0}")]
    RebuildFailed(String),
    #[error("Index lock acquisition failed: {0}")]
    LockAcquisitionFailed(String),
    #[error("Insufficient disk space for rebuild")]
    InsufficientDiskSpace,
    #[error("Rebuild interrupted")]
    RebuildInterrupted,
}

#[derive(Debug, thiserror::Error)]
pub enum ClearError {
    #[error("Index clear failed: {0}")]
    ClearFailed(String),
    #[error("Index is locked")]
    IndexLocked,
    #[error("Clear operation interrupted")]
    ClearInterrupted,
}

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("Index validation failed: {0}")]
    ValidationFailed(String),
    #[error("Index corruption detected")]
    IndexCorrupted,
    #[error("Schema validation failed: {0}")]
    SchemaValidationFailed(String),
}

// ConfigError is defined in ports.rs to avoid conflicts

#[derive(Debug, thiserror::Error)]
pub enum MaintenanceError {
    #[error("Maintenance task failed: {0}")]
    MaintenanceFailed(String),
    #[error("Task not supported: {0}")]
    TaskNotSupported(String),
    #[error("Maintenance interrupted")]
    MaintenanceInterrupted,
}

#[derive(Debug, thiserror::Error)]
pub enum SegmentError {
    #[error("Segment operation failed: {0}")]
    SegmentOperationFailed(String),
    #[error("Segment not found: {0}")]
    SegmentNotFound(String),
    #[error("Segment merge failed: {0}")]
    SegmentMergeFailed(String),
}

#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("Merge operation failed: {0}")]
    MergeFailed(String),
    #[error("Merge policy violation: {0}")]
    MergePolicyViolation(String),
    #[error("Merge interrupted")]
    MergeInterrupted,
}

// Convert to FullTextSearchError for consistent handling
impl From<RebuildError> for FullTextSearchError {
    fn from(err: RebuildError) -> Self {
        FullTextSearchError::IndexOperations { source: IndexError::Rebuild { source: err } }
    }
}

impl From<ClearError> for FullTextSearchError {
    fn from(err: ClearError) -> Self {
        FullTextSearchError::IndexOperations { source: IndexError::Clear { source: err } }
    }
}

impl From<ValidationError> for FullTextSearchError {
    fn from(err: ValidationError) -> Self {
        FullTextSearchError::IndexOperations { source: IndexError::Validate { source: err } }
    }
}

// Note: ConfigError is defined in ports.rs to avoid conflict
// Note: MaintenanceError, SegmentError, and MergeError already have #[from] attributes in the enum