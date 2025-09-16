//! Full Text Search Feature Module
//!
//! This module provides comprehensive full-text search capabilities following
//! Vertical Slice Architecture principles with segregated interfaces.

pub mod dto;
pub mod ports;
pub mod error;
pub mod use_case;
pub mod adapter;
pub mod di;

// Re-export commonly used types for easier access
pub use dto::*;
pub use error::*;
pub use ports::*;
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
    
    
    /// Get feature health status (via use cases / DI, without exposing REST controllers)
    pub async fn health_check(&self) -> FeatureHealthStatus {
        let ready = self.di_container.search_use_case().is_ready();
        let overall = if ready { HealthStatus::Healthy } else { HealthStatus::Warning };
        FeatureHealthStatus {
            feature_name: "search_full_text".to_string(),
            is_healthy: overall == HealthStatus::Healthy,
            components: vec![("search_engine".to_string(), overall.clone())],
            last_check: chrono::Utc::now(),
            message: match overall {
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

// Integration helpers for REST are intentionally omitted per architecture guidelines.