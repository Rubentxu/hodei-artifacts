// Search Crate

pub mod features {
    pub mod basic_search;
    pub mod index_text_documents;
    pub mod search_full_text;
    pub mod advanced_query;
}

// Re-export commonly used types for easier access
pub use features::basic_search::{
    BasicSearchDIContainer, SearchQuery, SearchResults, ArtifactDocument, BasicSearchError
};
pub use features::index_text_documents::{
    IndexTextDocumentsDIContainer, IndexDocumentCommand, DocumentIndexedResponse, IndexDocumentError
};
pub use features::search_full_text::{
    SearchFullTextDIContainer, SearchRequest, FullTextSearchResults, FullTextSearchError,
    SearchFullTextFeature, SearchFeatureConfig, FeatureHealthStatus, FeatureStatistics
};
pub use features::advanced_query::{
    AdvancedQueryDIContainer, AdvancedSearchQuery, AdvancedSearchResults, ParsedQueryInfo, AdvancedQueryError
};

// Common search types and utilities
pub mod domain;
pub mod error;

/// Search crate initialization
pub struct SearchFeature {
    pub basic_search: std::sync::Arc<features::basic_search::BasicSearchDIContainer>,
    pub index_text_documents: std::sync::Arc<features::index_text_documents::IndexTextDocumentsDIContainer>,
    pub search_full_text: std::sync::Arc<features::search_full_text::SearchFullTextDIContainer>,
    pub advanced_query: std::sync::Arc<features::advanced_query::AdvancedQueryDIContainer>,
}

impl SearchFeature {
    /// Create a new search feature with all sub-features
    pub fn new(
        basic_search: std::sync::Arc<features::basic_search::BasicSearchDIContainer>,
        index_text_documents: std::sync::Arc<features::index_text_documents::IndexTextDocumentsDIContainer>,
        search_full_text: std::sync::Arc<features::search_full_text::SearchFullTextDIContainer>,
        advanced_query: std::sync::Arc<features::advanced_query::AdvancedQueryDIContainer>,
    ) -> Self {
        Self {
            basic_search,
            index_text_documents,
            search_full_text,
            advanced_query,
        }
    }
    
    /// Initialize all search features
    pub async fn initialize(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!("Initializing Search feature");
        
        // Initialize each sub-feature
        // Note: individual features may have their own initialization logic
        // This is a placeholder for coordinated initialization
        
        tracing::info!("Search feature initialized successfully");
        Ok(())
    }
    
    /// Get health status for all search features
    pub async fn health_check(&self) -> SearchHealthStatus {
        // This would aggregate health status from all sub-features
        SearchHealthStatus {
            feature_name: "search".to_string(),
            is_healthy: true, // TODO: Aggregate from sub-features
            components: vec![
                ("basic_search".to_string(), shared::lifecycle::HealthStatus::Healthy),
                ("index_text_documents".to_string(), shared::lifecycle::HealthStatus::Healthy),
                ("search_full_text".to_string(), shared::lifecycle::HealthStatus::Healthy),
                ("advanced_query".to_string(), shared::lifecycle::HealthStatus::Healthy),
            ],
            last_check: chrono::Utc::now(),
            message: "All search features are healthy".to_string(),
        }
    }
}

/// Health status for the search feature
#[derive(Debug, Clone)]
pub struct SearchHealthStatus {
    pub feature_name: String,
    pub is_healthy: bool,
    pub components: Vec<(String, shared::lifecycle::HealthStatus)>,
    pub last_check: chrono::DateTime<chrono::Utc>,
    pub message: String,
}

