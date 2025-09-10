use async_trait::async_trait;
use crate::features::full_text_search::{
    dto::{FullTextSearchQuery, FullTextSearchResults, IndexedArtifact},
    error::FullTextSearchError,
};

/// Port for search engine functionality
#[async_trait]
pub trait SearchEnginePort: Send + Sync {
    /// Search for artifacts using full-text search
    async fn search(
        &self,
        query: &FullTextSearchQuery,
    ) -> Result<FullTextSearchResults, FullTextSearchError>;
    
    /// Get search suggestions based on partial query
    async fn get_suggestions(
        &self,
        partial_query: &str,
        limit: usize,
    ) -> Result<Vec<String>, FullTextSearchError>;
    
    /// Get search statistics
    async fn get_stats(&self) -> Result<SearchStats, FullTextSearchError>;
}

/// Port for indexing functionality
#[async_trait]
pub trait IndexerPort: Send + Sync {
    /// Index a single artifact
    async fn index_artifact(
        &self,
        artifact: &IndexedArtifact,
    ) -> Result<(), FullTextSearchError>;
    
    /// Index multiple artifacts in batch
    async fn index_artifacts_batch(
        &self,
        artifacts: &[IndexedArtifact],
    ) -> Result<BatchIndexingResult, FullTextSearchError>;
    
    /// Delete an artifact from the index
    async fn delete_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<(), FullTextSearchError>;
    
    /// Reindex all artifacts
    async fn reindex_all(&self) -> Result<ReindexingResult, FullTextSearchError>;
}

/// Port for text tokenization functionality
#[async_trait]
pub trait TokenizerPort: Send + Sync {
    /// Tokenize text content
    fn tokenize(&self, text: &str) -> Result<Vec<Token>, FullTextSearchError>;
    
    /// Detect language of text content
    fn detect_language(&self, text: &str) -> Result<String, FullTextSearchError>;
    
    /// Stem tokens for a specific language
    fn stem_tokens(&self, tokens: &[Token], language: &str) -> Result<Vec<Token>, FullTextSearchError>;
}

/// Port for relevance scoring functionality
#[async_trait]
pub trait ScorerPort: Send + Sync {
    /// Calculate relevance score for a document
    fn calculate_score(
        &self,
        query_terms: &[String],
        document_terms: &[String],
        document_length: usize,
    ) -> Result<f32, FullTextSearchError>;
    
    /// Normalize scores for a result set
    fn normalize_scores(&self, scores: &[f32]) -> Result<Vec<f32>, FullTextSearchError>;
}

/// Token representation
#[derive(Debug, Clone)]
pub struct Token {
    pub text: String,
    pub position: usize,
    pub start_offset: usize,
    pub end_offset: usize,
}

/// Search statistics
#[derive(Debug, Clone)]
pub struct SearchStats {
    pub total_documents: usize,
    pub total_terms: usize,
    pub average_document_length: f32,
    pub index_size_bytes: u64,
    pub last_indexed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Batch indexing result
#[derive(Debug, Clone)]
pub struct BatchIndexingResult {
    pub indexed_count: usize,
    pub failed_count: usize,
    pub errors: Vec<String>,
    pub duration_ms: u128,
}

/// Reindexing result
#[derive(Debug, Clone)]
pub struct ReindexingResult {
    pub total_processed: usize,
    pub successful: usize,
    pub failed: usize,
    pub duration_ms: u128,
}