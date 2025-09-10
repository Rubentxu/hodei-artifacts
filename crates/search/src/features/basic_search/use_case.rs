use std::sync::Arc;
use tracing::{info, debug, error};
use crate::features::basic_search::{
    dto::{SearchQuery, SearchResults},
    error::BasicSearchError,
    ports::{SearchIndexPort, EventPublisherPort},
};

pub struct BasicSearchUseCase {
    search_index: Arc<dyn SearchIndexPort>,
    event_publisher: Arc<dyn EventPublisherPort>,
}

impl BasicSearchUseCase {
    pub fn new(
        search_index: Arc<dyn SearchIndexPort>,
        event_publisher: Arc<dyn EventPublisherPort>,
    ) -> Self {
        Self {
            search_index,
            event_publisher,
        }
    }

    pub async fn execute(&self, query: SearchQuery) -> Result<SearchResults, BasicSearchError> {
        info!(query = %query.q, "Executing basic search");
        
        // Normalize query to be case-insensitive
        let normalized_query = query.q.to_lowercase();
        let search_query = SearchQuery {
            q: normalized_query.clone(),
            page: query.page,
            page_size: query.page_size,
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
}