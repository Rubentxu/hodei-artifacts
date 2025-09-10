use async_trait::async_trait;
use crate::features::full_text_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::FullTextSearchError,
};

/// Port for search index functionality
#[async_trait]
pub trait SearchIndexPort: Send + Sync {
    /// Search for artifacts using full-text search
    async fn search(
        &self,
        query: &SearchQuery,
    ) -> Result<SearchResults, FullTextSearchError>;
    
    /// Index a single artifact
    async fn index_artifact(
        &self,
        artifact: &ArtifactDocument,
    ) -> Result<(), FullTextSearchError>;
    
    /// Get all artifacts with pagination
    async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<SearchResults, FullTextSearchError>;
}

/// Port for artifact repository functionality
#[async_trait]
pub trait ArtifactRepositoryPort: Send + Sync {
    /// Get an artifact by its ID
    async fn get_artifact_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ArtifactDocument>, FullTextSearchError>;
    
    /// List all artifacts with pagination
    async fn list_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<(Vec<ArtifactDocument>, usize), FullTextSearchError>;
}

/// Port for event publisher functionality
#[async_trait]
pub trait EventPublisherPort: Send + Sync {
    /// Publish search query executed event
    async fn publish_search_query_executed(
        &self,
        query: &str,
        result_count: usize,
    ) -> Result<(), FullTextSearchError>;
    
    /// Publish search result clicked event
    async fn publish_search_result_clicked(
        &self,
        artifact_id: &str,
    ) -> Result<(), FullTextSearchError>;
}

/// Port for scorer functionality
#[async_trait]
pub trait ScorerPort: Send + Sync {
    /// Calculate relevance score for a document
    async fn calculate_score(
        &self,
        query_terms: &[String],
        document_terms: &[String],
        document_length: usize,
    ) -> Result<f32, FullTextSearchError>;
    
    /// Normalize scores for a result set
    async fn normalize_scores(
        &self,
        scores: &[f32],
    ) -> Result<Vec<f32>, FullTextSearchError>;
    
    /// Rank search results by relevance
    async fn rank_results(
        &self,
        results: &mut SearchResults,
    ) -> Result<(), FullTextSearchError>;
}