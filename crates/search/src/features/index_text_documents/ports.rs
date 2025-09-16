//! Segregated Ports for Index Text Documents Feature
//!
//! This module defines specific interfaces following the Interface Segregation Principle (ISP)
//! for document indexing operations. Each port has a single, well-defined responsibility.

use async_trait::async_trait;
use serde::{Serialize, Deserialize};
use std::sync::Arc;
use crate::features::index_text_documents::dto::*;

/// Port for document indexing operations
/// 
/// This port is responsible for the core functionality of adding, updating,
/// and removing documents from the search index.
#[async_trait]
pub trait DocumentIndexerPort: Send + Sync {
    /// Index a single document in the search engine
    async fn index_document(&self, command: IndexDocumentCommand) -> Result<DocumentIndexedResponse, IndexError>;
    
    /// Index multiple documents in a batch operation
    async fn batch_index_documents(&self, command: BatchIndexCommand) -> Result<BatchIndexResponse, IndexError>;
    
    /// Remove a document from the search index
    async fn remove_document(&self, command: RemoveDocumentCommand) -> Result<DocumentRemovedResponse, IndexError>;
    
    /// Get information about indexed documents
    async fn get_indexed_documents(&self, query: GetIndexedDocumentsQuery) -> Result<IndexedDocumentsResponse, IndexError>;
    
    /// Check if a document exists in the index
    async fn document_exists(&self, document_id: &str) -> Result<bool, IndexError>;
}

/// Port for text analysis and preprocessing
/// 
/// This port handles linguistic analysis including tokenization, stemming,
/// stop word removal, and language detection.
#[async_trait]
pub trait TextAnalyzerPort: Send + Sync {
    /// Analyze text content before indexing
    async fn analyze_text(&self, command: AnalyzeTextCommand) -> Result<TextAnalysisResponse, AnalysisError>;
    
    /// Extract tokens from text with basic processing
    async fn extract_tokens(&self, text: &str, language: Option<&str>) -> Result<Vec<TokenInfo>, TokenizationError>;
    
    /// Apply stemming to a list of tokens
    async fn apply_stemming(&self, tokens: Vec<TokenInfo>, language: &str) -> Result<Vec<TokenInfo>, StemmingError>;
    
    /// Remove stop words from token list
    async fn remove_stop_words(&self, tokens: Vec<TokenInfo>, language: &str) -> Result<Vec<TokenInfo>, StopWordError>;
    
    /// Detect the language of the given text
    async fn detect_language(&self, text: &str) -> Result<Option<String>, LanguageDetectionError>;
}

/// Port for index health monitoring and statistics
/// 
/// This port provides monitoring capabilities for the search index,
/// including health checks, performance metrics, and statistics.
#[async_trait]
pub trait IndexHealthMonitorPort: Send + Sync {
    /// Check the overall health of the search index
    async fn check_index_health(&self) -> Result<IndexHealth, HealthError>;
    
    /// Get detailed statistics about the search index
    async fn get_index_stats(&self) -> Result<IndexStats, StatsError>;
    
    /// Get performance metrics for indexing operations
    async fn get_indexing_performance_metrics(&self, time_range: TimeRange) -> Result<IndexingMetrics, MetricsError>;
    
    /// Monitor memory usage of the search index
    async fn get_memory_usage(&self) -> Result<MemoryUsage, MemoryError>;
    
    /// Check if the index needs optimization
    async fn needs_optimization(&self) -> Result<bool, OptimizationError>;
}

/// Port for index schema management
/// 
/// This port handles the creation and management of the search index schema,
/// including field definitions and index configurations.
#[async_trait]
pub trait IndexSchemaManagerPort: Send + Sync {
    /// Create or update the search index schema
    async fn create_schema(&self, config: SchemaConfig) -> Result<SchemaInfo, SchemaError>;
    
    /// Get the current schema configuration
    async fn get_schema(&self) -> Result<SchemaInfo, SchemaError>;
    
    /// Add a new field to the index schema
    async fn add_field(&self, field_config: FieldConfig) -> Result<FieldInfo, SchemaError>;
    
    /// Update an existing field in the schema
    async fn update_field(&self, field_name: &str, field_config: FieldConfig) -> Result<FieldInfo, SchemaError>;
    
    /// Validate the current schema
    async fn validate_schema(&self) -> Result<SchemaValidationResult, SchemaError>;
}

/// Port for document validation
/// 
/// This port handles validation of documents before indexing,
/// ensuring they meet the required format and constraints.
#[async_trait]
pub trait DocumentValidatorPort: Send + Sync {
    /// Validate a document before indexing
    async fn validate_document(&self, command: &IndexDocumentCommand) -> Result<ValidationResult, ValidationError>;
    
    /// Validate document metadata
    async fn validate_metadata(&self, metadata: &ArtifactMetadata) -> Result<MetadataValidationResult, ValidationError>;
    
    /// Validate text content for indexing
    async fn validate_content(&self, content: &str) -> Result<ContentValidationResult, ValidationError>;
    
    /// Check for duplicate content
    async fn check_duplicate_content(&self, content: &str) -> Result<bool, DuplicateError>;
}

/// Error types for indexing operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum IndexError {
    #[error("Failed to index document: {0}")]
    IndexingFailed(String),
    
    #[error("Document not found: {0}")]
    DocumentNotFound(String),
    
    #[error("Index operation timed out: {0}ms")]
    Timeout(u64),
    
    #[error("Index is currently unavailable")]
    IndexUnavailable,
    
    #[error("Schema validation failed: {0}")]
    SchemaError(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Configuration error: {0}")]
    ConfigurationError(String),
    
    #[error("Unknown error: {0}")]
    Unknown(String),
}

/// Error types for text analysis
#[derive(Debug, Clone, thiserror::Error)]
pub enum AnalysisError {
    #[error("Text analysis failed: {0}")]
    AnalysisFailed(String),
    
    #[error("Unsupported language: {0}")]
    UnsupportedLanguage(String),
    
    #[error("Text processing timeout: {0}ms")]
    Timeout(u64),
    
    #[error("Invalid analysis options: {0}")]
    InvalidOptions(String),
}

/// Error types for tokenization
#[derive(Debug, Clone, thiserror::Error)]
pub enum TokenizationError {
    #[error("Tokenization failed: {0}")]
    TokenizationFailed(String),
    
    #[error("Empty text provided for tokenization")]
    EmptyText,
    
    #[error("Token size exceeds limits: {0}")]
    TokenTooLarge(String),
}

/// Error types for stemming
#[derive(Debug, Clone, thiserror::Error)]
pub enum StemmingError {
    #[error("Stemming failed: {0}")]
    StemmingFailed(String),
    
    #[error("Unsupported language for stemming: {0}")]
    UnsupportedLanguage(String),
    
    #[error("Stemmer not available for language: {0}")]
    StemmerNotAvailable(String),
}

/// Error types for stop word removal
#[derive(Debug, Clone, thiserror::Error)]
pub enum StopWordError {
    #[error("Stop word removal failed: {0}")]
    StopWordRemovalFailed(String),
    
    #[error("Stop word list not available for language: {0}")]
    StopWordListNotAvailable(String),
}

/// Error types for language detection
#[derive(Debug, Clone, thiserror::Error)]
pub enum LanguageDetectionError {
    #[error("Language detection failed: {0}")]
    DetectionFailed(String),
    
    #[error("Insufficient text for language detection")]
    InsufficientText,
    
    #[error("Language detection confidence too low: {0}")]
    LowConfidence(f32),
}

/// Error types for health monitoring
#[derive(Debug, Clone, thiserror::Error)]
pub enum HealthError {
    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),
    
    #[error("Index is in unhealthy state")]
    UnhealthyIndex,
    
    #[error("Health check timeout: {0}ms")]
    Timeout(u64),
}

/// Error types for statistics
#[derive(Debug, Clone, thiserror::Error)]
pub enum StatsError {
    #[error("Failed to get index stats: {0}")]
    StatsRetrievalFailed(String),
    
    #[error("Statistics not available")]
    StatsNotAvailable,
}

/// Error types for metrics
#[derive(Debug, Clone, thiserror::Error)]
pub enum MetricsError {
    #[error("Failed to get performance metrics: {0}")]
    MetricsRetrievalFailed(String),
    
    #[error("Invalid time range for metrics")]
    InvalidTimeRange,
}

/// Error types for memory monitoring
#[derive(Debug, Clone, thiserror::Error)]
pub enum MemoryError {
    #[error("Failed to get memory usage: {0}")]
    MemoryUsageFailed(String),
    
    #[error("Memory usage exceeds limits: {0}MB")]
    MemoryLimitExceeded(u64),
}

/// Error types for optimization
#[derive(Debug, Clone, thiserror::Error)]
pub enum OptimizationError {
    #[error("Failed to check optimization needs: {0}")]
    OptimizationCheckFailed(String),
    
    #[error("Optimization check timeout: {0}ms")]
    Timeout(u64),
}

/// Error types for schema management
#[derive(Debug, Clone, thiserror::Error)]
pub enum SchemaError {
    #[error("Schema creation failed: {0}")]
    SchemaCreationFailed(String),
    
    #[error("Schema validation failed: {0}")]
    SchemaValidationFailed(String),
    
    #[error("Field already exists: {0}")]
    FieldAlreadyExists(String),
    
    #[error("Field not found: {0}")]
    FieldNotFound(String),
    
    #[error("Invalid field configuration: {0}")]
    InvalidFieldConfig(String),
}

/// Error types for document validation
#[derive(Debug, Clone, thiserror::Error)]
pub enum ValidationError {
    #[error("Document validation failed: {0}")]
    ValidationFailed(String),
    
    #[error("Required field missing: {0}")]
    RequiredFieldMissing(String),
    
    #[error("Invalid field value: {0}")]
    InvalidFieldValue(String),
    
    #[error("Document size exceeds limits: {0} bytes")]
    DocumentTooLarge(u64),
}

/// Error types for duplicate detection
#[derive(Debug, Clone, thiserror::Error)]
pub enum DuplicateError {
    #[error("Duplicate detection failed: {0}")]
    DuplicateCheckFailed(String),
    
    #[error("Duplicate content detected")]
    DuplicateContent,
}

/// Index health information
#[derive(Debug, Clone)]
pub struct IndexHealth {
    /// Overall health status
    pub status: HealthStatus,
    /// Number of documents in the index
    pub document_count: u64,
    /// Index size in bytes
    pub index_size_bytes: u64,
    /// Memory usage in bytes
    pub memory_usage_bytes: u64,
    /// Last update timestamp
    pub last_updated: chrono::DateTime<chrono::Utc>,
    /// Health status details
    pub details: Vec<HealthDetail>,
}

/// Health status enum
#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    /// Index is healthy
    Healthy,
    /// Index has warnings but is functional
    Warning,
    /// Index is unhealthy and needs attention
    Unhealthy,
    /// Index status is unknown
    Unknown,
}

/// Health detail information
#[derive(Debug, Clone)]
pub struct HealthDetail {
    /// Name of the health check
    pub name: String,
    /// Status of the check
    pub status: HealthStatus,
    /// Message describing the check result
    pub message: String,
    /// Timestamp of the check
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Index statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    /// Total number of documents
    pub total_documents: u64,
    /// Total number of terms in the index
    pub total_terms: u64,
    /// Average number of terms per document
    pub avg_terms_per_document: f64,
    /// Index size in bytes
    pub index_size_bytes: u64,
    /// Memory usage in bytes
    pub memory_usage_bytes: u64,
    /// Number of segments in the index
    pub segment_count: u32,
    /// Index creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last optimization timestamp
    pub last_optimized_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Time range for metrics queries
#[derive(Debug, Clone)]
pub struct TimeRange {
    /// Start of the time range
    pub start: chrono::DateTime<chrono::Utc>,
    /// End of the time range
    pub end: chrono::DateTime<chrono::Utc>,
}

/// Indexing performance metrics
#[derive(Debug, Clone)]
pub struct IndexingMetrics {
    /// Average indexing time in milliseconds
    pub avg_indexing_time_ms: f64,
    /// Total number of indexing operations
    pub total_operations: u64,
    /// Number of successful operations
    pub successful_operations: u64,
    /// Number of failed operations
    pub failed_operations: u64,
    /// Operations per second
    pub operations_per_second: f64,
    /// P99 latency in milliseconds
    pub p99_latency_ms: f64,
}

/// Memory usage information
#[derive(Debug, Clone)]
pub struct MemoryUsage {
    /// Current memory usage in bytes
    pub current_usage_bytes: u64,
    /// Peak memory usage in bytes
    pub peak_usage_bytes: u64,
    /// Memory limit in bytes
    pub memory_limit_bytes: u64,
    /// Memory usage percentage
    pub usage_percentage: f64,
}

/// Schema configuration
#[derive(Debug, Clone)]
pub struct SchemaConfig {
    /// Name of the schema
    pub name: String,
    /// Field configurations
    pub fields: Vec<FieldConfig>,
    /// Index settings
    pub settings: IndexSettings,
}

/// Field configuration
#[derive(Debug, Clone)]
pub struct FieldConfig {
    /// Name of the field
    pub name: String,
    /// Type of the field
    pub field_type: FieldType,
    /// Whether the field is indexed
    pub indexed: bool,
    /// Whether the field is stored
    pub stored: bool,
    /// Whether the field is required
    pub required: bool,
    /// Field-specific options
    pub options: FieldOptions,
}

/// Field type enum
#[derive(Debug, Clone)]
pub enum FieldType {
    /// Text field for full-text search
    Text,
    /// String field for exact matching
    String,
    /// Integer field
    Integer,
    /// Float field
    Float,
    /// Boolean field
    Boolean,
    /// Date field
    Date,
    /// JSON field
    Json,
}

/// Field options
#[derive(Debug, Clone)]
pub struct FieldOptions {
    /// Whether to tokenize the field
    pub tokenize: bool,
    /// Whether to apply stemming
    pub stem: bool,
    /// Whether to store term vectors
    pub store_term_vectors: bool,
    /// Analyzer to use for the field
    pub analyzer: Option<String>,
}

/// Index settings
#[derive(Debug, Clone)]
pub struct IndexSettings {
    /// Number of shards
    pub number_of_shards: u32,
    /// Number of replicas
    pub number_of_replicas: u32,
    /// Refresh interval
    pub refresh_interval: String,
    /// Analysis settings
    pub analysis: AnalysisSettings,
}

/// Analysis settings
#[derive(Debug, Clone)]
pub struct AnalysisSettings {
    /// Analyzer configurations
    pub analyzers: Vec<AnalyzerConfig>,
    /// Tokenizer configurations
    pub tokenizers: Vec<TokenizerConfig>,
    /// Filter configurations
    pub filters: Vec<FilterConfig>,
}

/// Analyzer configuration
#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    /// Name of the analyzer
    pub name: String,
    /// Type of the analyzer
    pub analyzer_type: String,
    /// Tokenizer to use
    pub tokenizer: String,
    /// Filters to apply
    pub filters: Vec<String>,
}

/// Tokenizer configuration
#[derive(Debug, Clone)]
pub struct TokenizerConfig {
    /// Name of the tokenizer
    pub name: String,
    /// Type of the tokenizer
    pub tokenizer_type: String,
    /// Tokenizer-specific settings
    pub settings: std::collections::HashMap<String, String>,
}

/// Filter configuration
#[derive(Debug, Clone)]
pub struct FilterConfig {
    /// Name of the filter
    pub name: String,
    /// Type of the filter
    pub filter_type: String,
    /// Filter-specific settings
    pub settings: std::collections::HashMap<String, String>,
}

/// Schema information
#[derive(Debug, Clone)]
pub struct SchemaInfo {
    /// Schema name
    pub name: String,
    /// Schema version
    pub version: String,
    /// Field definitions
    pub fields: Vec<FieldInfo>,
    /// Schema settings
    pub settings: IndexSettings,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Field information
#[derive(Debug, Clone)]
pub struct FieldInfo {
    /// Field name
    pub name: String,
    /// Field type
    pub field_type: FieldType,
    /// Whether the field is indexed
    pub indexed: bool,
    /// Whether the field is stored
    pub stored: bool,
    /// Whether the field is required
    pub required: bool,
    /// Field options
    pub options: FieldOptions,
}

/// Schema validation result
#[derive(Debug, Clone)]
pub struct SchemaValidationResult {
    /// Whether the schema is valid
    pub is_valid: bool,
    /// Validation errors
    pub errors: Vec<String>,
    /// Validation warnings
    pub warnings: Vec<String>,
}

/// Document validation result
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Whether the document is valid
    pub is_valid: bool,
    /// Validation errors
    pub errors: Vec<String>,
    /// Validation warnings
    pub warnings: Vec<String>,
}

/// Metadata validation result
#[derive(Debug, Clone)]
pub struct MetadataValidationResult {
    /// Whether the metadata is valid
    pub is_valid: bool,
    /// Validation errors
    pub errors: Vec<String>,
    /// Validation warnings
    pub warnings: Vec<String>,
}

/// Content validation result
#[derive(Debug, Clone)]
pub struct ContentValidationResult {
    /// Whether the content is valid
    pub is_valid: bool,
    /// Content length
    pub content_length: usize,
    /// Validation errors
    pub errors: Vec<String>,
    /// Validation warnings
    pub warnings: Vec<String>,
}