use async_trait::async_trait;
use tracing::{info, debug, error};

use crate::features::basic_search::{
    error::BasicSearchError,
    ports::EventPublisherPort,
};

/// Production adapter for event publishing
/// This is a simplified implementation that just logs events
/// In a real implementation, this would connect to RabbitMQ or another message broker
pub struct LoggingEventPublisherAdapter;

impl LoggingEventPublisherAdapter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl EventPublisherPort for LoggingEventPublisherAdapter {
    async fn publish_search_query_executed(
        &self,
        query: &str,
        result_count: usize,
    ) -> Result<(), BasicSearchError> {
        info!("SearchQueryExecuted: query='{}', result_count={}", query, result_count);
        Ok(())
    }
    
    async fn publish_search_result_clicked(
        &self,
        artifact_id: &str,
    ) -> Result<(), BasicSearchError> {
        info!("SearchResultClicked: artifact_id='{}'", artifact_id);
        Ok(())
    }
}