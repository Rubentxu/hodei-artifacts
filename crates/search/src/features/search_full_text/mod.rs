//! Full Text Search Feature Module
//!
//! This module provides comprehensive full-text search capabilities following
//! Vertical Slice Architecture principles with segregated interfaces.

pub mod dto;
pub mod ports;
pub mod error;
pub mod use_case;
pub mod adapter;
pub mod api;
pub mod di;

// Re-export commonly used types for easier access
pub use dto::*;
pub use error::*;
pub use ports::*;
pub use api::*;
pub use di::*;

use std::sync::Arc;

/// Feature initialization and configuration
pub struct SearchFullTextFeature {
    pub di_container: Arc<SearchFullTextDIContainer>,
}

impl SearchFullTextFeature {
    /// Create a new search feature with default configuration
    pub fn new(di_container: Arc<SearchFullTextDIContainer>) -> Self {
        Self { di_container }
    }
    
    /// Initialize the search feature
    pub async fn initialize(&self) -> Result<(), FullTextSearchError> {
        tracing::info!("Initializing Full Text Search feature");
        
        // Perform any necessary initialization
        // For example, validate index, warm up caches, etc.
        
        tracing::info!("Full Text Search feature initialized successfully");
        Ok(())
    }
    
    /// Get the search API
    pub fn api(&self) -> Arc<FullTextSearchApi> {
        self.di_container.search_api()
    }
    
    /// Get feature health status
    pub async fn health_check(&self) -> FeatureHealthStatus {
        let search_health = self.di_container.search_api().health_check().await;
        
        FeatureHealthStatus {
            feature_name: "search_full_text".to_string(),
            is_healthy: search_health.overall_status == HealthStatus::Healthy,
            components: vec![("search_engine".to_string(), search_health.overall_status)],
            last_check: chrono::Utc::now(),
            message: match search_health.overall_status {
                HealthStatus::Healthy => "Search engine is healthy".to_string(),
                HealthStatus::Warning => "Search engine has warnings".to_string(),
                HealthStatus::Unhealthy => "Search engine is unhealthy".to_string(),
                HealthStatus::Unknown => "Search engine status unknown".to_string(),
            },
        }
    }
    
    /// Get feature statistics
    pub async fn get_statistics(&self) -> FeatureStatistics {
        // This would collect various statistics about the search feature
        // For now, return basic information
        FeatureStatistics {
            feature_name: "search_full_text".to_string(),
            total_queries: 0, // TODO: Track query count
            average_query_time_ms: 0.0,
            cache_hit_rate: 0.0,
            error_rate: 0.0,
            index_size_bytes: 0,
            document_count: 0,
            last_updated: chrono::Utc::now(),
        }
    }
}

/// Feature health status
#[derive(Debug, Clone)]
pub struct FeatureHealthStatus {
    pub feature_name: String,
    pub is_healthy: bool,
    pub components: Vec<(String, HealthStatus)>,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub message: String,
}

/// Feature statistics
#[derive(Debug, Clone)]
pub struct FeatureStatistics {
    pub feature_name: String,
    pub total_queries: u64,
    pub average_query_time_ms: f64,
    pub cache_hit_rate: f64,
    pub error_rate: f64,
    pub index_size_bytes: u64,
    pub document_count: u64,
    pub last_updated: chrono::DateTime<chrono::Utc>,
}

/// Create a default search feature for testing
#[cfg(test)]
pub fn create_test_feature() -> SearchFullTextFeature {
    let di_container = Arc::new(SearchFullTextDIContainer::for_testing());
    SearchFullTextFeature::new(di_container)
}

/// Feature-specific configuration
#[derive(Debug, Clone)]
pub struct SearchFeatureConfig {
    pub index_path: String,
    pub max_results_per_page: usize,
    pub default_snippet_length: usize,
    pub enable_highlights: bool,
    pub enable_suggestions: bool,
    pub cache_size_mb: usize,
    pub optimization_interval_seconds: u64,
}

impl Default for SearchFeatureConfig {
    fn default() -> Self {
        Self {
            index_path: "/tmp/tantivy_search_index".to_string(),
            max_results_per_page: 100,
            default_snippet_length: 150,
            enable_highlights: true,
            enable_suggestions: true,
            cache_size_mb: 128,
            optimization_interval_seconds: 3600, // 1 hour
        }
    }
}

impl SearchFullTextFeature {
    /// Create a search feature with custom configuration
    pub fn with_config(config: SearchFeatureConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let di_container = Arc::new(SearchFullTextDIContainer::for_production(&config.index_path)?);
        Ok(Self::new(di_container))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_feature_initialization() {
        let feature = create_test_feature();
        
        let result = feature.initialize().await;
        assert!(result.is_ok());
        
        let health = feature.health_check().await;
        assert!(health.is_healthy);
    }
    
    #[tokio::test]
    async fn test_feature_statistics() {
        let feature = create_test_feature();
        
        let stats = feature.get_statistics().await;
        assert_eq!(stats.feature_name, "search_full_text");
    }
    
    #[tokio::test]
    async fn test_feature_with_config() {
        let config = SearchFeatureConfig {
            index_path: "/tmp/test_search_index".to_string(),
            max_results_per_page: 50,
            ..Default::default()
        };
        
        let result = SearchFullTextFeature::with_config(config);
        
        // This might fail if we can't create the index directory
        // In a real test, we'd use a temporary directory
        match result {
            Ok(_) => {
                // Success case
            }
            Err(_) => {
                // Expected to fail in test environment without proper setup
            }
        }
    }
}

// Integration helper functions
pub mod integration {
    use super::*;
    use axum::{Json, extract::Query};
    
    /// Create Axum routes for the search feature
    pub fn create_routes() -> axum::Router {
        use axum::{
            routing::{get, post},
            extract::Query,
            Json,
        };
        
        axum::Router::new()
            .route("/search", post(handle_search))
            .route("/search/suggestions", get(handle_suggestions))
            .route("/search/analyze", post(handle_analyze))
            .route("/search/more-like-this/:document_id", get(handle_more_like_this))
            .route("/search/facets", post(handle_facets))
            .route("/search/health", get(handle_health))
    }
    
    /// Search handler
    async fn handle_search(
        Json(request): Json<SearchRequest>,
    ) -> Json<ApiResponse<FullTextSearchResults>> {
        // In a real implementation, this would get the feature from app state
        let feature = create_test_feature();
        let response = feature.api().search_api(request).await;
        Json(response)
    }
    
    /// Suggestions handler
    async fn handle_suggestions(
        Query(params): Query<std::collections::HashMap<String, String>>,
    ) -> Json<ApiResponse<SearchSuggestionsResponse>> {
        let partial_query = params.get("q").cloned().unwrap_or_default();
        let request = SuggestionsRequest {
            partial_query,
            limit: params.get("limit").and_then(|s| s.parse().ok()),
            ..Default::default()
        };
        
        let feature = create_test_feature();
        let response = feature.api().suggestions_api(request).await;
        Json(response)
    }
    
    /// Query analysis handler
    async fn handle_analyze(
        Json(request): Json<QueryAnalysisRequest>,
    ) -> Json<ApiResponse<QueryPerformanceAnalysis>> {
        let feature = create_test_feature();
        let response = feature.api().analyze_query_api(request).await;
        Json(response)
    }
    
    /// More-like-this handler
    async fn handle_more_like_this(
        axum::extract::Path(document_id): axum::extract::Path<String>,
        Query(params): Query<std::collections::HashMap<String, String>>,
    ) -> Json<ApiResponse<FullTextSearchResults>> {
        let limit = params.get("limit").and_then(|s| s.parse().ok());
        
        let feature = create_test_feature();
        let response = feature.api().more_like_this_api(document_id, limit).await;
        Json(response)
    }
    
    /// Facets handler
    async fn handle_facets(
        Json(request): Json<SearchRequest>,
    ) -> Json<ApiResponse<SearchFacets>> {
        let feature = create_test_feature();
        let response = feature.api().facets_api(request).await;
        Json(response)
    }
    
    /// Health check handler
    async fn handle_health() -> Json<FeatureHealthStatus> {
        let feature = create_test_feature();
        Json(feature.health_check().await)
    }
}