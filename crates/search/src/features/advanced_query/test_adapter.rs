use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tokio::sync::Mutex;

use crate::features::basic_search::{
    dto::{ArtifactDocument},
    error::BasicSearchError,
    ports::{SearchIndexPort, ArtifactRepositoryPort, EventPublisherPort},
};

use crate::features::advanced_query::{
    dto::{AdvancedSearchQuery, AdvancedSearchResults, ParsedQueryInfo},
    error::AdvancedQueryError,
    ports::{QueryParserPort, AdvancedSearchIndexPort, AdvancedArtifactRepositoryPort, AdvancedEventPublisherPort, QueryParsingStats},
};

/// Mock query parser adapter for testing
#[derive(Debug, Clone)]
pub struct MockQueryParserAdapter {
    pub parsed_queries: Arc<RwLock<HashMap<String, ParsedQueryInfo>>>,
}

impl MockQueryParserAdapter {
    pub fn new() -> Self {
        Self {
            parsed_queries: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn add_test_parsed_query(&self, query: &str, parsed: ParsedQueryInfo) {
        let mut parsed_queries = self.parsed_queries.write().unwrap();
        parsed_queries.insert(query.to_string(), parsed);
    }
}

#[async_trait]
impl QueryParserPort for MockQueryParserAdapter {
    async fn parse(
        &self,
        query: &str,
    ) -> Result<ParsedQueryInfo, AdvancedQueryError> {
        let parsed_queries = self.parsed_queries.read().unwrap();
        if let Some(parsed) = parsed_queries.get(query) {
            Ok(parsed.clone())
        } else {
            // For testing, we'll create a simple parsed query
            Ok(ParsedQueryInfo {
                original_query: query.to_string(),
                parsed_fields: vec![],
                boolean_operators: vec![],
                has_wildcards: false,
                has_fuzzy: false,
                has_ranges: false,
            })
        }
    }
    
    async fn validate(
        &self,
        query: &str,
    ) -> Result<bool, AdvancedQueryError> {
        // For testing, we'll assume all queries are valid
        Ok(true)
    }
    
    async fn get_stats(&self) -> Result<QueryParsingStats, AdvancedQueryError> {
        Ok(QueryParsingStats {
            total_parsed: 0,
            parse_errors: 0,
            avg_parse_time_ms: 0.0,
            max_parse_time_ms: 0,
            min_parse_time_ms: 0,
        })
    }
}

/// Mock advanced search index adapter for testing
#[derive(Debug, Clone)]
pub struct MockAdvancedSearchIndexAdapter {
    pub artifacts: Arc<RwLock<Vec<ArtifactDocument>>>,
}

impl MockAdvancedSearchIndexAdapter {
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
impl AdvancedSearchIndexPort for MockAdvancedSearchIndexAdapter {
    async fn search(
        &self,
        query: &AdvancedSearchQuery,
    ) -> Result<AdvancedSearchResults, AdvancedQueryError> {
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
        
        // Create parsed query info
        let parsed_query_info = ParsedQueryInfo {
            original_query: query.q.clone(),
            parsed_fields: vec![],
            boolean_operators: vec![],
            has_wildcards: false,
            has_fuzzy: false,
            has_ranges: false,
        };
        
        Ok(AdvancedSearchResults::new(paginated, total_count, page, page_size, parsed_query_info))
    }
    
    async fn index_artifact(
        &self,
        artifact: &ArtifactDocument,
    ) -> Result<(), AdvancedQueryError> {
        let mut artifacts = self.artifacts.write().unwrap();
        artifacts.push(artifact.clone());
        Ok(())
    }
    
    async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<AdvancedSearchResults, AdvancedQueryError> {
        let artifacts = self.artifacts.read().unwrap();
        let total_count = artifacts.len();
        
        let offset = (page - 1) * page_size;
        let paginated = if offset < total_count {
            let end = std::cmp::min(offset + page_size, total_count);
            artifacts[offset..end].to_vec()
        } else {
            Vec::new()
        };
        
        // Create parsed query info
        let parsed_query_info = ParsedQueryInfo {
            original_query: "".to_string(),
            parsed_fields: vec![],
            boolean_operators: vec![],
            has_wildcards: false,
            has_fuzzy: false,
            has_ranges: false,
        };
        
        Ok(AdvancedSearchResults::new(paginated, total_count, page, page_size, parsed_query_info))
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
impl AdvancedEventPublisherPort for MockEventPublisherAdapter {
    async fn publish_advanced_search_query_executed(
        &self,
        query: &str,
        parsed_query: &ParsedQueryInfo,
        result_count: usize,
        query_time_ms: u128,
    ) -> Result<(), AdvancedQueryError> {
        let event = format!(
            "AdvancedSearchQueryExecuted: query='{}', parsed_fields={:?}, result_count={}, query_time_ms={}",
            query, parsed_query.parsed_fields, result_count, query_time_ms
        );
        let mut events = self.events.write().unwrap();
        events.push(event);
        Ok(())
    }
    
    async fn publish_advanced_search_result_clicked(
        &self,
        artifact_id: &str,
        query: &str,
    ) -> Result<(), AdvancedQueryError> {
        let event = format!(
            "AdvancedSearchResultClicked: artifact_id='{}', query='{}'",
            artifact_id, query
        );
        let mut events = self.events.write().unwrap();
        events.push(event);
        Ok(())
    }
}