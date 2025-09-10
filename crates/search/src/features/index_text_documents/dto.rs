//! Data Transfer Objects for Index Text Documents Feature
//!
//! This module contains all the DTOs for document indexing operations,
//! following VSA principles with segregated interfaces.

use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::collections::HashMap;

/// Command to index a document with full-text search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDocumentCommand {
    /// Unique identifier for the artifact
    pub artifact_id: String,
    /// Raw text content to index
    pub content: String,
    /// Structured metadata for the artifact
    pub metadata: ArtifactMetadata,
    /// Language code for text analysis (e.g., "en", "es", "fr")
    pub language: Option<String>,
    /// Whether to force re-indexing if already exists
    pub force_reindex: bool,
}

/// Response after successful document indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentIndexedResponse {
    /// Unique identifier for the indexed document
    pub document_id: String,
    /// Time taken for indexing in milliseconds
    pub indexing_time_ms: u64,
    /// Current status of the indexing operation
    pub status: IndexingStatus,
    /// Number of tokens extracted and indexed
    pub token_count: usize,
    /// Index operation identifier
    pub operation_id: String,
}

/// Command for batch indexing multiple documents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchIndexCommand {
    /// List of documents to index
    pub documents: Vec<IndexDocumentCommand>,
    /// Whether to process in parallel
    pub parallel_processing: bool,
    /// Maximum number of concurrent operations
    pub max_concurrency: Option<usize>,
}

/// Response for batch indexing operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchIndexResponse {
    /// Results for each individual document
    pub results: Vec<DocumentIndexedResponse>,
    /// Overall batch operation status
    pub batch_status: BatchOperationStatus,
    /// Total time for batch operation
    pub total_time_ms: u64,
    /// Number of successfully indexed documents
    pub success_count: usize,
    /// Number of failed documents
    pub failure_count: usize,
}

/// Command to remove a document from index
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveDocumentCommand {
    /// Document identifier to remove
    pub document_id: String,
    /// Whether to remove all associated metadata
    pub remove_metadata: bool,
}

/// Response after document removal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentRemovedResponse {
    /// Document identifier that was removed
    pub document_id: String,
    /// Status of the removal operation
    pub status: RemovalStatus,
    /// Time taken for removal
    pub removal_time_ms: u64,
}

/// Artifact metadata structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    /// Title of the artifact
    pub title: Option<String>,
    /// Description of the artifact
    pub description: Option<String>,
    /// Tags associated with the artifact
    pub tags: Vec<String>,
    /// Artifact type (e.g., "jar", "npm", "docker")
    pub artifact_type: String,
    /// Version of the artifact
    pub version: String,
    /// Additional custom metadata
    pub custom_metadata: HashMap<String, String>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last modification timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Query to get indexed documents
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetIndexedDocumentsQuery {
    /// Filter by artifact type
    pub artifact_type: Option<String>,
    /// Filter by language
    pub language: Option<String>,
    /// Filter by tags
    pub tags: Option<Vec<String>>,
    /// Pagination page number
    pub page: Option<usize>,
    /// Number of items per page
    pub page_size: Option<usize>,
}

/// Response for indexed documents query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDocumentsResponse {
    /// List of indexed documents
    pub documents: Vec<IndexedDocumentInfo>,
    /// Total number of documents matching the query
    pub total_count: usize,
    /// Current page number
    pub page: usize,
    /// Number of items per page
    pub page_size: usize,
}

/// Information about an indexed document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedDocumentInfo {
    /// Document identifier
    pub document_id: String,
    /// Artifact metadata
    pub metadata: ArtifactMetadata,
    /// Language used for indexing
    pub language: Option<String>,
    /// Number of tokens in the document
    pub token_count: usize,
    /// Indexing status
    pub status: IndexingStatus,
    /// Last indexed timestamp
    pub last_indexed_at: chrono::DateTime<chrono::Utc>,
}

/// Status of indexing operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum IndexingStatus {
    /// Document successfully indexed
    Completed,
    /// Indexing in progress
    InProgress,
    /// Indexing failed
    Failed,
    /// Document queued for indexing
    Queued,
    /// Document removed from index
    Removed,
}

/// Status of batch operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BatchOperationStatus {
    /// All documents processed successfully
    Completed,
    /// Some documents failed, others succeeded
    PartialSuccess,
    /// All documents failed
    Failed,
    /// Batch operation was cancelled
    Cancelled,
}

/// Status of document removal operations
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RemovalStatus {
    /// Document successfully removed
    Removed,
    /// Document was not found in index
    NotFound,
    /// Removal operation failed
    Failed,
    /// Removal operation in progress
    InProgress,
}

/// Command to analyze text before indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeTextCommand {
    /// Text content to analyze
    pub text: String,
    /// Language code for analysis
    pub language: Option<String>,
    /// Analysis options
    pub options: TextAnalysisOptions,
}

/// Options for text analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextAnalysisOptions {
    /// Whether to perform stemming
    pub enable_stemming: bool,
    /// Whether to remove stop words
    pub remove_stop_words: bool,
    /// Whether to perform tokenization
    pub enable_tokenization: bool,
    /// Minimum token length to keep
    pub min_token_length: Option<usize>,
    /// Maximum token length to keep
    pub max_token_length: Option<usize>,
}

/// Response for text analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextAnalysisResponse {
    /// Original text
    pub original_text: String,
    /// Analyzed language
    pub detected_language: Option<String>,
    /// Extracted tokens
    pub tokens: Vec<TokenInfo>,
    /// Number of tokens extracted
    pub token_count: usize,
    /// Analysis time in milliseconds
    pub analysis_time_ms: u64,
}

/// Information about a token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// The token text
    pub token: String,
    /// Position of the token in the original text
    pub position: usize,
    /// Token frequency in the document
    pub frequency: usize,
    /// Whether the token is a stop word
    pub is_stop_word: bool,
    /// Stemmed version of the token
    pub stemmed: Option<String>,
}

impl Default for TextAnalysisOptions {
    fn default() -> Self {
        Self {
            enable_stemming: true,
            remove_stop_words: true,
            enable_tokenization: true,
            min_token_length: Some(2),
            max_token_length: Some(100),
        }
    }
}

impl Default for ArtifactMetadata {
    fn default() -> Self {
        Self {
            title: None,
            description: None,
            tags: Vec::new(),
            artifact_type: String::new(),
            version: String::new(),
            custom_metadata: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}

impl ArtifactMetadata {
    /// Create test metadata for unit tests
    pub fn test_data() -> Self {
        Self {
            title: Some("Test Artifact".to_string()),
            description: Some("Test artifact description".to_string()),
            tags: vec!["test".to_string(), "sample".to_string()],
            artifact_type: "jar".to_string(),
            version: "1.0.0".to_string(),
            custom_metadata: HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }
}

impl IndexDocumentCommand {
    /// Create a test command for unit tests
    pub fn test_data() -> Self {
        Self {
            artifact_id: Uuid::new_v4().to_string(),
            content: "This is a test document content for indexing purposes.".to_string(),
            metadata: ArtifactMetadata::test_data(),
            language: Some("en".to_string()),
            force_reindex: false,
        }
    }
}