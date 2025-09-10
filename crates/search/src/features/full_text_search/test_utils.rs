use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

use crate::features::full_text_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::FullTextSearchError,
    ports::{SearchIndexPort, ArtifactRepositoryPort, EventPublisherPort, ScorerPort},
};

/// Mock search index adapter for testing
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

#[async_trait]
impl SearchIndexPort for MockSearchIndexAdapter {
    async fn search(
        &self,
        query: &SearchQuery,
    ) -> Result<SearchResults, FullTextSearchError> {
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
    ) -> Result<(), FullTextSearchError> {
        let mut artifacts = self.artifacts.write().unwrap();
        artifacts.push(artifact.clone());
        Ok(())
    }
    
    async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<SearchResults, FullTextSearchError> {
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

/// Mock artifact repository adapter for testing
#[derive(Debug, Clone)]
pub struct MockArtifactRepositoryAdapter {
    pub artifacts: Arc<RwLock<HashMap<String, ArtifactDocument>>>,
}

impl MockArtifactRepositoryAdapter {
    pub fn new() -> Self {
        Self {
            artifacts: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn add_test_artifact(&self, artifact: ArtifactDocument) {
        let mut artifacts = self.artifacts.write().unwrap();
        artifacts.insert(artifact.id.clone(), artifact);
    }
}

#[async_trait]
impl ArtifactRepositoryPort for MockArtifactRepositoryAdapter {
    async fn get_artifact_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ArtifactDocument>, FullTextSearchError> {
        let artifacts = self.artifacts.read().unwrap();
        Ok(artifacts.get(id).cloned())
    }
    
    async fn list_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<(Vec<ArtifactDocument>, usize), FullTextSearchError> {
        let artifacts = self.artifacts.read().unwrap();
        let all_artifacts: Vec<ArtifactDocument> = artifacts.values().cloned().collect();
        let total_count = all_artifacts.len();
        
        let offset = (page - 1) * page_size;
        let paginated = if offset < total_count {
            let end = std::cmp::min(offset + page_size, total_count);
            all_artifacts[offset..end].to_vec()
        } else {
            Vec::new()
        };
        
        Ok((paginated, total_count))
    }
}

/// Mock event publisher adapter for testing
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
    
    pub async fn get_published_events(&self) -> Vec<String> {
        let events = self.events.read().unwrap();
        events.clone()
    }
}

#[async_trait]
impl EventPublisherPort for MockEventPublisherAdapter {
    async fn publish_search_query_executed(
        &self,
        query: &str,
        result_count: usize,
    ) -> Result<(), FullTextSearchError> {
        let event = format!("SearchQueryExecuted: query='{}', result_count={}", query, result_count);
        let mut events = self.events.write().unwrap();
        events.push(event);
        Ok(())
    }
    
    async fn publish_search_result_clicked(
        &self,
        artifact_id: &str,
    ) -> Result<(), FullTextSearchError> {
        let event = format!("SearchResultClicked: artifact_id='{}'", artifact_id);
        let mut events = self.events.write().unwrap();
        events.push(event);
        Ok(())
    }
}

/// Mock scorer adapter for testing
#[derive(Debug, Clone)]
pub struct MockScorerAdapter {
    pub scores: Arc<RwLock<Vec<f32>>>,
}

impl MockScorerAdapter {
    pub fn new() -> Self {
        Self {
            scores: Arc::new(RwLock::new(Vec::new())),
        }
    }
    
    pub async fn add_test_scores(&self, scores: Vec<f32>) {
        let mut mock_scores = self.scores.write().unwrap();
        mock_scores.extend(scores);
    }
}

#[async_trait]
impl ScorerPort for MockScorerAdapter {
    async fn calculate_score(
        &self,
        _query_terms: &[String],
        _document_terms: &[String],
        _document_length: usize,
    ) -> Result<f32, FullTextSearchError> {
        let scores = self.scores.read().unwrap();
        Ok(*scores.first().unwrap_or(&1.0))
    }
    
    async fn normalize_scores(&self, scores: &[f32]) -> Result<Vec<f32>, FullTextSearchError> {
        Ok(scores.to_vec())
    }
    
    async fn rank_results(
        &self,
        results: &mut SearchResults,
    ) -> Result<(), FullTextSearchError> {
        // For testing, we'll just sort by score descending
        results.artifacts.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(())
    }
}