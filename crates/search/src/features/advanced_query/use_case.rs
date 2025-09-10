use std::sync::Arc;
use tracing::{info, debug, error};

use crate::features::advanced_query::{
    dto::{AdvancedSearchQuery, AdvancedSearchResults, ParsedQueryInfo},
    error::AdvancedQueryError,
    ports::{QueryParserPort, AdvancedSearchIndexPort, AdvancedEventPublisherPort},
};

pub struct AdvancedQueryUseCase {
    query_parser: Arc<dyn QueryParserPort>,
    search_index: Arc<dyn AdvancedSearchIndexPort>,
    event_publisher: Arc<dyn AdvancedEventPublisherPort>,
}

impl AdvancedQueryUseCase {
    pub fn new(
        query_parser: Arc<dyn QueryParserPort>,
        search_index: Arc<dyn AdvancedSearchIndexPort>,
        event_publisher: Arc<dyn AdvancedEventPublisherPort>,
    ) -> Self {
        Self {
            query_parser,
            search_index,
            event_publisher,
        }
    }

    pub async fn execute(&self, query: AdvancedSearchQuery) -> Result<AdvancedSearchResults, AdvancedQueryError> {
        info!(query = %query.q, "Executing advanced search");
        
        // Record start time for performance metrics
        let start_time = std::time::Instant::now();
        
        // Parse and validate the query
        let parsed_query = self.query_parser.parse(&query.q).await?;
        
        // Validate the query
        let is_valid = self.query_parser.validate(&query.q).await?;
        if !is_valid {
            return Err(AdvancedQueryError::QueryParseError("Invalid query syntax".to_string()));
        }
        
        // Execute the search
        let results = self.search_index.search(&query).await?;
        
        // Calculate query time
        let query_time_ms = start_time.elapsed().as_millis();
        
        // Publish search event
        if let Err(e) = self.event_publisher
            .publish_advanced_search_query_executed(&query.q, &parsed_query, results.total_count, query_time_ms)
            .await
        {
            error!(error = %e, "Failed to publish advanced search query executed event");
            // We don't return an error here as the search itself was successful
        }
        
        info!(result_count = results.total_count, query_time_ms = query_time_ms, "Advanced search completed successfully");
        Ok(results.with_query_time(query_time_ms))
    }
    
    pub async fn index_artifact(&self, artifact: &crate::features::basic_search::dto::ArtifactDocument) -> Result<(), AdvancedQueryError> {
        debug!(artifact_id = %artifact.id, "Indexing artifact in advanced search");
        
        self.search_index.index_artifact(artifact).await?;
        
        info!(artifact_id = %artifact.id, "Artifact indexed successfully");
        Ok(())
    }
    
    pub async fn get_all_artifacts(&self, page: usize, page_size: usize) -> Result<AdvancedSearchResults, AdvancedQueryError> {
        debug!(page = page, page_size = page_size, "Getting all artifacts in advanced search");
        
        let results = self.search_index.get_all_artifacts(page, page_size).await?;
        
        info!(result_count = results.total_count, "All artifacts retrieved successfully");
        Ok(results)
    }
}