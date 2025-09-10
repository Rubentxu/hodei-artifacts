use async_trait::async_trait;
use crate::features::advanced_query::{
    dto::{AdvancedSearchQuery, AdvancedSearchResults, ParsedQueryInfo},
    error::AdvancedQueryError,
};

/// Port for advanced query parsing functionality
#[async_trait]
pub trait QueryParserPort: Send + Sync {
    /// Parse an advanced query string into a structured representation
    async fn parse(
        &self,
        query: &str,
    ) -> Result<ParsedQueryInfo, AdvancedQueryError>;
    
    /// Validate an advanced query string
    async fn validate(
        &self,
        query: &str,
    ) -> Result<bool, AdvancedQueryError>;
    
    /// Get query parsing statistics
    async fn get_stats(&self) -> Result<QueryParsingStats, AdvancedQueryError>;
}

/// Port for advanced search index functionality
#[async_trait]
pub trait AdvancedSearchIndexPort: Send + Sync {
    /// Search for artifacts using advanced query syntax
    async fn search(
        &self,
        query: &AdvancedSearchQuery,
    ) -> Result<AdvancedSearchResults, AdvancedQueryError>;
    
    /// Index a single artifact with advanced metadata
    async fn index_artifact(
        &self,
        artifact: &crate::features::basic_search::dto::ArtifactDocument,
    ) -> Result<(), AdvancedQueryError>;
    
    /// Get all artifacts with pagination
    async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<AdvancedSearchResults, AdvancedQueryError>;
}

/// Port for advanced artifact repository functionality
#[async_trait]
pub trait AdvancedArtifactRepositoryPort: Send + Sync {
    /// Get an artifact by its ID with advanced metadata
    async fn get_artifact_by_id(
        &self,
        id: &str,
    ) -> Result<Option<crate::features::basic_search::dto::ArtifactDocument>, AdvancedQueryError>;
    
    /// List all artifacts with pagination and advanced filtering
    async fn list_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
        filter: Option<&str>,
    ) -> Result<(Vec<crate::features::basic_search::dto::ArtifactDocument>, usize), AdvancedQueryError>;
}

/// Port for advanced event publisher functionality
#[async_trait]
pub trait AdvancedEventPublisherPort: Send + Sync {
    /// Publish advanced search query executed event
    async fn publish_advanced_search_query_executed(
        &self,
        query: &str,
        parsed_query: &ParsedQueryInfo,
        result_count: usize,
        query_time_ms: u128,
    ) -> Result<(), AdvancedQueryError>;
    
    /// Publish advanced search result clicked event
    async fn publish_advanced_search_result_clicked(
        &self,
        artifact_id: &str,
        query: &str,
    ) -> Result<(), AdvancedQueryError>;
}

/// Query parsing statistics
#[derive(Debug, Clone)]
pub struct QueryParsingStats {
    pub total_parsed: usize,
    pub parse_errors: usize,
    pub avg_parse_time_ms: f64,
    pub max_parse_time_ms: u128,
    pub min_parse_time_ms: u128,
}