use std::sync::Arc;
use tracing::{info, debug, error};

use crate::features::full_text_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::FullTextSearchError,
    ports::{SearchIndexPort, ArtifactRepositoryPort, EventPublisherPort},
};

pub struct FullTextSearchUseCase {
    search_index: Arc<dyn SearchIndexPort>,
    artifact_repository: Arc<dyn ArtifactRepositoryPort>,
    event_publisher: Arc<dyn EventPublisherPort>,
}

impl FullTextSearchUseCase {
    pub fn new(
        search_index: Arc<dyn SearchIndexPort>,
        artifact_repository: Arc<dyn ArtifactRepositoryPort>,
        event_publisher: Arc<dyn EventPublisherPort>,
    ) -> Self {
        Self {
            search_index,
            artifact_repository,
            event_publisher,
        }
    }

    pub async fn execute(&self, query: SearchQuery) -> Result<SearchResults, FullTextSearchError> {
        info!(query = %query.q, "Executing full-text search");
        
        // Normalize query to be case-insensitive
        let normalized_query = query.q.to_lowercase();
        let search_query = SearchQuery {
            q: normalized_query.clone(),
            page: query.page,
            page_size: query.page_size,
            language: query.language,
            fields: query.fields,
        };
        
        // Handle empty search case
        let results = if normalized_query.is_empty() {
            debug!("Empty search query, returning all artifacts");
            let page = query.page.unwrap_or(1);
            let page_size = query.page_size.unwrap_or(20);
            self.search_index.get_all_artifacts(page, page_size).await?
        } else {
            debug!(query = %normalized_query, "Performing search with query");
            self.search_index.search(&search_query).await?
        };
        
        // Publish search event
        if let Err(e) = self.event_publisher
            .publish_search_query_executed(&normalized_query, results.total_count)
            .await
        {
            error!(error = %e, "Failed to publish search query executed event");
            // We don't return an error here as the search itself was successful
        }
        
        info!(result_count = results.total_count, "Search completed successfully");
        Ok(results)
    }
    
    pub async fn index_artifact(&self, artifact: &ArtifactDocument) -> Result<(), FullTextSearchError> {
        debug!(artifact_id = %artifact.id, "Indexing artifact in full-text search");
        
        self.search_index.index_artifact(artifact).await?;
        
        info!(artifact_id = %artifact.id, "Artifact indexed successfully");
        Ok(())
    }
    
    pub async fn get_all_artifacts(&self, page: usize, page_size: usize) -> Result<SearchResults, FullTextSearchError> {
        debug!(page = page, page_size = page_size, "Getting all artifacts");
        
        self.search_index.get_all_artifacts(page, page_size).await
    }
}