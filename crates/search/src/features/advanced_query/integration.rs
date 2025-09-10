use std::sync::Arc;
use tracing::info;

use crate::features::basic_search::{
    dto::{SearchQuery, ArtifactDocument},
    error::BasicSearchError,
    ports::{SearchIndexPort, ArtifactRepositoryPort, EventPublisherPort},
};

use crate::features::advanced_query::{
    dto::{AdvancedSearchQuery, AdvancedSearchResults, ParsedQueryInfo},
    error::AdvancedQueryError,
};

pub struct AdvancedQueryIntegration {
    basic_search_index: Arc<dyn SearchIndexPort>,
    artifact_repository: Arc<dyn ArtifactRepositoryPort>,
    event_publisher: Arc<dyn EventPublisherPort>,
}

impl AdvancedQueryIntegration {
    pub fn new(
        basic_search_index: Arc<dyn SearchIndexPort>,
        artifact_repository: Arc<dyn ArtifactRepositoryPort>,
        event_publisher: Arc<dyn EventPublisherPort>,
    ) -> Self {
        Self {
            basic_search_index,
            artifact_repository,
            event_publisher,
        }
    }

    pub async fn search(
        &self,
        query: AdvancedSearchQuery,
    ) -> Result<AdvancedSearchResults, AdvancedQueryError> {
        info!(query = %query.q, "Executing advanced search");
        
        // Transform to basic search query for compatibility
        let basic_query = SearchQuery {
            q: query.q.clone(),
            page: query.page,
            page_size: query.page_size,
            // Note: We're omitting the language field since it doesn't exist in the basic search query
        };
        
        // Execute search using existing infrastructure
        let basic_results = self.basic_search_index
            .search(&basic_query)
            .await
            .map_err(|e| AdvancedQueryError::SearchExecutionError(format!("Failed to execute search: {}", e)))?;
        
        // Convert to advanced search results
        let parsed_query_info = ParsedQueryInfo {
            original_query: query.q.clone(),
            parsed_fields: vec![],
            boolean_operators: vec![],
            has_wildcards: false,
            has_fuzzy: false,
            has_ranges: false,
        };
        
        let advanced_results = AdvancedSearchResults::new(
            basic_results.artifacts,
            basic_results.total_count,
            basic_results.page,
            basic_results.page_size,
            parsed_query_info,
        );
        
        // Publish search event
        if let Err(e) = self.event_publisher
            .publish_search_query_executed(&query.q, basic_results.total_count)
            .await
        {
            // We don't return an error here as the search itself was successful
        }
        
        info!(result_count = basic_results.total_count, "Advanced search completed successfully");
        Ok(advanced_results)
    }
    
    pub async fn index_artifact(
        &self,
        artifact: &ArtifactDocument,
    ) -> Result<(), AdvancedQueryError> {
        self.basic_search_index.index_artifact(artifact).await
            .map_err(|e| AdvancedQueryError::SearchExecutionError(format!("Failed to index artifact: {}", e)))
    }
    
    pub async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<AdvancedSearchResults, AdvancedQueryError> {
        // For an empty query, we'll return all artifacts
        // This is a simplified implementation - in reality, we'd want to use a MatchAllQuery
        // and implement proper pagination
        
        let basic_results = self.basic_search_index
            .get_all_artifacts(page, page_size)
            .await
            .map_err(|e| AdvancedQueryError::SearchExecutionError(format!("Failed to get all artifacts: {}", e)))?;
        
        // Convert to advanced search results
        let parsed_query_info = ParsedQueryInfo {
            original_query: "".to_string(),
            parsed_fields: vec![],
            boolean_operators: vec![],
            has_wildcards: false,
            has_fuzzy: false,
            has_ranges: false,
        };
        
        let advanced_results = AdvancedSearchResults::new(
            basic_results.artifacts,
            basic_results.total_count,
            basic_results.page,
            basic_results.page_size,
            parsed_query_info,
        );
        
        Ok(advanced_results)
    }
}