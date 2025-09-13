//! HTTP API for Full Text Search Feature
//!
//! This module provides the HTTP endpoints for search operations,
//! following the VSA principle of having focused, single-purpose API components.

use std::sync::Arc;
use tracing::{debug, info, warn, error, instrument};
use serde::{Deserialize, Serialize};

use super::use_case::*;
use super::dto::*;
use super::error::{FullTextSearchError, ToFullTextSearchError};
use super::ports::*;

/// HTTP API for full-text search operations
pub struct FullTextSearchApi {
    search_use_case: Arc<FullTextSearchUseCase>,
    suggestions_use_case: Arc<SearchSuggestionsUseCase>,
    query_analysis_use_case: Arc<QueryPerformanceUseCase>,
}

impl FullTextSearchApi {
    pub fn new(
        search_use_case: Arc<FullTextSearchUseCase>,
        suggestions_use_case: Arc<SearchSuggestionsUseCase>,
        query_analysis_use_case: Arc<QueryPerformanceUseCase>,
    ) -> Self {
        Self {
            search_use_case,
            suggestions_use_case,
            query_analysis_use_case,
        }
    }
    
    /// Execute a full-text search query
    #[instrument(skip(self, query))]
    pub async fn search(&self, query: FullTextSearchQuery) -> Result<FullTextSearchResults, FullTextSearchError> {
        debug!("Executing search query: {}", query.q);
        
        let start_time = std::time::Instant::now();
        let result = self.search_use_case.execute(query).await;
        
        match &result {
            Ok(results) => {
                info!(
                    query_time_ms = results.query_time_ms,
                    total_results = results.total_count,
                    "Search query completed successfully"
                );
            }
            Err(e) => {
                error!(
                    error = %e,
                    query_time_ms = start_time.elapsed().as_millis(),
                    "Search query failed"
                );
            }
        }
        
        result
    }
    
    /// Get search suggestions for a partial query
    #[instrument(skip(self, query))]
    pub async fn get_suggestions(&self, query: SearchSuggestionsQuery) -> Result<SearchSuggestionsResponse, FullTextSearchError> {
        debug!("Getting suggestions for: {}", query.partial_query);
        
        self.suggestions_use_case.execute(query).await.map_err(|e| {
            error!("Failed to get suggestions: {}", e);
            e.to_full_text_search_error()
        })
    }
    
    /// Analyze query performance
    #[instrument(skip(self, command))]
    pub async fn analyze_query(&self, command: AnalyzeQueryPerformanceCommand) -> Result<QueryPerformanceAnalysis, FullTextSearchError> {
        debug!("Analyzing query performance for: {}", command.query.q);
        
        self.query_analysis_use_case.execute(command).await.map_err(|e| {
            error!("Failed to analyze query: {}", e);
            e.to_full_text_search_error()
        })
    }
    
    /// Get similar documents (more-like-this)
    #[instrument(skip(self, document_id))]
    pub async fn more_like_this(
        &self,
        document_id: String,
        limit: Option<usize>,
    ) -> Result<FullTextSearchResults, FullTextSearchError> {
        debug!("Getting similar documents for: {}", document_id);
        
        let limit = limit.unwrap_or(10);
        self.search_use_case.more_like_this(&document_id, limit).await
    }
    
    /// Get search facets
    #[instrument(skip(self, query))]
    pub async fn get_facets(&self, query: FullTextSearchQuery) -> Result<SearchFacets, FullTextSearchError> {
        debug!("Getting facets for query: {}", query.q);
        
        self.search_use_case.get_facets(query).await.map_err(|e| {
            error!("Failed to get facets: {}", e);
            e.to_full_text_search_error()
        })
    }
    
    /// Health check for search functionality
    pub async fn health_check(&self) -> SearchHealthStatus {
        debug!("Performing search health check");
        
        // Execute a simple query to verify search is working
        let test_query = FullTextSearchQuery {
            q: "test".to_string(),
            page_size: Some(1),
            ..Default::default()
        };
        
        match self.search_use_case.execute(test_query).await {
            Ok(_) => SearchHealthStatus {
                overall_status: HealthStatus::Healthy,
                components: vec![],
                last_check: chrono::Utc::now(),
            },
            Err(e) => {
                warn!("Search health check failed: {}", e);
                SearchHealthStatus {
                    overall_status: HealthStatus::Unhealthy,
                    components: vec![],
                    last_check: chrono::Utc::now(),
                }
            }
        }
    }
}

/// HTTP request/response types for search operations
#[derive(Debug, Serialize, Deserialize)]
pub struct SearchRequest {
    pub query: String,
    pub artifact_type: Option<String>,
    pub language: Option<String>,
    pub tags: Option<Vec<String>>,
    pub date_range: Option<DateRange>,
    pub search_mode: Option<SearchMode>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub include_highlights: Option<bool>,
    pub include_snippets: Option<bool>,
    pub snippet_length: Option<usize>,
    pub sort_order: Option<SortOrder>,
    pub min_score: Option<f32>,
    pub fuzziness: Option<u32>,
    pub enable_stemming: Option<bool>,
    pub enable_phonetic: Option<bool>,
}

impl From<SearchRequest> for FullTextSearchQuery {
    fn from(request: SearchRequest) -> Self {
        Self {
            q: request.query,
            artifact_type: request.artifact_type,
            language: request.language,
            tags: request.tags,
            date_range: request.date_range,
            search_mode: request.search_mode.unwrap_or(SearchMode::Simple),
            page: request.page,
            page_size: request.page_size,
            include_highlights: request.include_highlights.unwrap_or(true),
            include_snippets: request.include_snippets.unwrap_or(true),
            snippet_length: request.snippet_length,
            sort_order: request.sort_order.unwrap_or(SortOrder::Relevance),
            min_score: request.min_score,
            fuzziness: request.fuzziness,
            enable_stemming: request.enable_stemming,
            enable_phonetic: request.enable_phonetic,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SuggestionsRequest {
    pub partial_query: String,
    pub limit: Option<usize>,
    pub suggestion_types: Option<Vec<SuggestionType>>,
    pub context: Option<String>,
    pub language: Option<String>,
}

impl From<SuggestionsRequest> for SearchSuggestionsQuery {
    fn from(request: SuggestionsRequest) -> Self {
        Self {
            partial_query: request.partial_query,
            limit: request.limit,
            suggestion_types: request.suggestion_types,
            context: request.context,
            language: request.language,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueryAnalysisRequest {
    pub query: SearchRequest,
    pub include_timing: Option<bool>,
    pub include_execution_plan: Option<bool>,
    pub include_index_stats: Option<bool>,
}

impl From<QueryAnalysisRequest> for AnalyzeQueryPerformanceCommand {
    fn from(request: QueryAnalysisRequest) -> Self {
        Self {
            query: request.query.into(),
            include_timing: request.include_timing.unwrap_or(true),
            include_execution_plan: request.include_execution_plan.unwrap_or(false),
            include_index_stats: request.include_index_stats.unwrap_or(false),
        }
    }
}

/// API response wrappers for better HTTP handling
#[derive(Debug, Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<ApiError>,
    pub metadata: Option<ResponseMetadata>,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub details: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ResponseMetadata {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub request_id: String,
    pub execution_time_ms: u64,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            data: Some(data),
            error: None,
            metadata: Some(ResponseMetadata {
                timestamp: chrono::Utc::now(),
                request_id: uuid::Uuid::new_v4().to_string(),
                execution_time_ms: 0,
            }),
        }
    }
    
    pub fn error(code: String, message: String) -> Self {
        Self {
            success: false,
            data: None,
            error: Some(ApiError {
                code,
                message,
                details: None,
            }),
            metadata: Some(ResponseMetadata {
                timestamp: chrono::Utc::now(),
                request_id: uuid::Uuid::new_v4().to_string(),
                execution_time_ms: 0,
            }),
        }
    }
}

impl FullTextSearchApi {
    /// Convert internal results to API response format
    pub fn to_api_response<T>(&self, result: Result<T, FullTextSearchError>) -> ApiResponse<T> {
        match result {
            Ok(data) => ApiResponse::success(data),
            Err(e) => ApiResponse::error(
                "SEARCH_ERROR".to_string(),
                e.to_string(),
            ),
        }
    }
    
    /// Execute search with API response format
    pub async fn search_api(&self, request: SearchRequest) -> ApiResponse<FullTextSearchResults> {
        let query: FullTextSearchQuery = request.into();
        let result = self.search(query).await;
        self.to_api_response(result)
    }
    
    /// Get suggestions with API response format
    pub async fn suggestions_api(&self, request: SuggestionsRequest) -> ApiResponse<SearchSuggestionsResponse> {
        let query: SearchSuggestionsQuery = request.into();
        let result = self.get_suggestions(query).await;
        self.to_api_response(result)
    }
    
    /// Analyze query with API response format
    pub async fn analyze_query_api(&self, request: QueryAnalysisRequest) -> ApiResponse<QueryPerformanceAnalysis> {
        let command: AnalyzeQueryPerformanceCommand = request.into();
        let result = self.analyze_query(command).await;
        self.to_api_response(result)
    }
    
    /// Get similar documents with API response format
    pub async fn more_like_this_api(
        &self,
        document_id: String,
        limit: Option<usize>,
    ) -> ApiResponse<FullTextSearchResults> {
        let result = self.more_like_this(document_id, limit).await;
        self.to_api_response(result)
    }
    
    /// Get facets with API response format
    pub async fn facets_api(&self, request: SearchRequest) -> ApiResponse<SearchFacets> {
        let query: FullTextSearchQuery = request.into();
        let result = self.get_facets(query).await;
        self.to_api_response(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::search_full_text::adapter::test::*;
    
    #[tokio::test]
    async fn test_search_api_success() {
        let use_case = Arc::new(MockFullTextSearchUseCase::new());
        let api = FullTextSearchApi::new(use_case, Arc::new(MockSearchSuggestionsUseCase::new()), Arc::new(MockQueryPerformanceUseCase::new()));
        
        let request = SearchRequest {
            query: "test".to_string(),
            page_size: Some(10),
            ..Default::default()
        };
        
        let response = api.search_api(request).await;
        assert!(response.success);
        assert!(response.data.is_some());
    }
    
    #[tokio::test]
    async fn test_suggestions_api_success() {
        let use_case = Arc::new(MockSearchSuggestionsUseCase::new());
        let api = FullTextSearchApi::new(Arc::new(MockFullTextSearchUseCase::new()), use_case, Arc::new(MockQueryPerformanceUseCase::new()));
        
        let request = SuggestionsRequest {
            partial_query: "test".to_string(),
            limit: Some(5),
            ..Default::default()
        };
        
        let response = api.suggestions_api(request).await;
        assert!(response.success);
        assert!(response.data.is_some());
    }
    
    #[tokio::test]
    async fn test_health_check() {
        let api = FullTextSearchApi::new(
            Arc::new(MockFullTextSearchUseCase::new()),
            Arc::new(MockSearchSuggestionsUseCase::new()),
            Arc::new(MockQueryPerformanceUseCase::new()),
        );
        
        let health = api.health_check().await;
        assert_eq!(health.overall_status, HealthStatus::Healthy);
    }
}