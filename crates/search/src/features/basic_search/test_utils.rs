use std::sync::{Arc, RwLock};
use async_trait::async_trait;

use crate::features::basic_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::BasicSearchError,
    ports::{SearchIndexPort, EventPublisherPort},
};

// Mock search index adapter for testing
#[derive(Debug, Clone)]
pub struct MockSearchIndexAdapter {
    pub artifacts: Arc<RwLock<Vec<ArtifactDocument>>>,
}

impl MockSearchIndexAdapter {
    pub fn new() -> Self {
        Self {
            artifacts: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    pub async fn add_test_artifact(&self, artifact: ArtifactDocument) {
        let mut artifacts = self.artifacts.write().unwrap();
        artifacts.push(artifact);
    }
}

impl Default for MockSearchIndexAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchIndexPort for MockSearchIndexAdapter {
    async fn search(
        &self,
        query: &SearchQuery,
    ) -> Result<SearchResults, BasicSearchError> {
        let artifacts = self.artifacts.read().unwrap();
        
        // Filter artifacts based on query
        let filtered: Vec<ArtifactDocument> = if query.q.is_empty() {
            artifacts.clone()
        } else {
            let query_lower = query.q.to_lowercase();
            artifacts.iter()
                .filter(|artifact| {
                    artifact.name.to_lowercase().contains(&query_lower) ||
                    artifact.version.to_lowercase().contains(&query_lower)
                })
                .cloned()
                .collect()
        };
        
        // Apply pagination
        let page = query.page.unwrap_or(1);
        let page_size = query.page_size.unwrap_or(20);
        let offset = (page - 1) * page_size;
        let total_count = filtered.len();
        
        let paginated = if offset < total_count {
            let end = std::cmp::min(offset + page_size, total_count);
            filtered[offset..end].to_vec()
        } else {
            Vec::new()
        };
        
        Ok(SearchResults::new(paginated, total_count, page, page_size))
    }
    
    async fn index_artifact(
        &self,
        artifact: &ArtifactDocument,
    ) -> Result<(), BasicSearchError> {
        let mut artifacts = self.artifacts.write().unwrap();
        artifacts.push(artifact.clone());
        Ok(())
    }
    
    async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<SearchResults, BasicSearchError> {
        let artifacts = self.artifacts.read().unwrap();
        let total_count = artifacts.len();
        
        let offset = (page - 1) * page_size;
        let paginated = if offset < total_count {
            let end = std::cmp::min(offset + page_size, total_count);
            artifacts[offset..end].to_vec()
        } else {
            Vec::new()
        };
        
        Ok(SearchResults::new(paginated, total_count, page, page_size))
    }
}

// Mock event publisher adapter for testing
#[derive(Debug, Clone)]
pub struct MockEventPublisherAdapter {
    pub events: Arc<RwLock<Vec<String>>>,
}

impl MockEventPublisherAdapter {
    pub fn new() -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    pub fn get_published_events(&self) -> Vec<String> {
        let events = self.events.read().unwrap();
        events.clone()
    }
}

impl Default for MockEventPublisherAdapter {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventPublisherPort for MockEventPublisherAdapter {
    async fn publish_search_query_executed(
        &self,
        query: &str,
        result_count: usize,
    ) -> Result<(), BasicSearchError> {
        let event = format!("SearchQueryExecuted: query='{}', result_count={}", query, result_count);
        let mut events = self.events.write().unwrap();
        events.push(event);
        Ok(())
    }
    
    async fn publish_search_result_clicked(
        &self,
        artifact_id: &str,
    ) -> Result<(), BasicSearchError> {
        let event = format!("SearchResultClicked: artifact_id='{}'", artifact_id);
        let mut events = self.events.write().unwrap();
        events.push(event);
        Ok(())
    }
}