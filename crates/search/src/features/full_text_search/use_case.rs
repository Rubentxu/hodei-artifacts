use std::sync::Arc;
use tracing::{info, debug, error};
use crate::features::full_text_search::{
    ports::{SearchEnginePort, IndexerPort},
    dto::{FullTextSearchQuery, FullTextSearchResults, IndexedArtifact},
    error::FullTextSearchError,
};

/// Use case for full-text search functionality
pub struct FullTextSearchUseCase {
    search_engine: Arc<dyn SearchEnginePort>,
    indexer: Arc<dyn IndexerPort>,
}

impl FullTextSearchUseCase {
    pub fn new(
        search_engine: Arc<dyn SearchEnginePort>,
        indexer: Arc<dyn IndexerPort>,
    ) -> Self {
        Self {
            search_engine,
            indexer,
        }
    }

    /// Execute a full-text search query
    pub async fn execute(
        &self,
        query: FullTextSearchQuery,
    ) -> Result<FullTextSearchResults, FullTextSearchError> {
        info!(query = %query.q, "Executing full-text search");
        
        // Validate query
        if query.q.trim().is_empty() {
            return Err(FullTextSearchError::InvalidQueryError(
                "Search query cannot be empty".to_string()
            ));
        }
        
        // Perform search
        let results = self.search_engine.search(&query).await?;
        
        // Log search performance
        debug!(
            query_time_ms = results.query_time_ms,
            result_count = results.total_count,
            max_score = results.max_score,
            "Search completed"
        );
        
        info!(result_count = results.total_count, "Full-text search completed successfully");
        Ok(results)
    }
    
    /// Index a single artifact
    pub async fn index_artifact(
        &self,
        artifact: IndexedArtifact,
    ) -> Result<(), FullTextSearchError> {
        debug!(artifact_id = %artifact.id, "Indexing artifact");
        
        self.indexer.index_artifact(&artifact).await?;
        
        info!(artifact_id = %artifact.id, "Artifact indexed successfully");
        Ok(())
    }
    
    /// Index multiple artifacts in batch
    pub async fn index_artifacts_batch(
        &self,
        artifacts: Vec<IndexedArtifact>,
    ) -> Result<(), FullTextSearchError> {
        debug!(artifact_count = artifacts.len(), "Indexing artifacts batch");
        
        if artifacts.is_empty() {
            return Ok(());
        }
        
        let result = self.indexer.index_artifacts_batch(&artifacts).await?;
        
        info!(
            indexed_count = result.indexed_count,
            failed_count = result.failed_count,
            "Batch indexing completed"
        );
        
        if result.failed_count > 0 {
            error!(failed_count = result.failed_count, "Some artifacts failed to index");
            // We don't return an error here as partial success is acceptable
        }
        
        Ok(())
    }
    
    /// Get search suggestions for a partial query
    pub async fn get_suggestions(
        &self,
        partial_query: &str,
        limit: usize,
    ) -> Result<Vec<String>, FullTextSearchError> {
        debug!(partial_query = %partial_query, limit = limit, "Getting search suggestions");
        
        let suggestions = self.search_engine.get_suggestions(partial_query, limit).await?;
        
        info!(suggestion_count = suggestions.len(), "Suggestions retrieved successfully");
        Ok(suggestions)
    }
    
    /// Get search engine statistics
    pub async fn get_stats(&self) -> Result<crate::features::full_text_search::ports::SearchStats, FullTextSearchError> {
        debug!("Getting search engine statistics");
        
        let stats = self.search_engine.get_stats().await?;
        
        info!(
            total_documents = stats.total_documents,
            index_size_bytes = stats.index_size_bytes,
            "Search engine statistics retrieved"
        );
        
        Ok(stats)
    }
}