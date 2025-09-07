use async_trait::async_trait;
use crate::domain::model::ArtifactSearchDocument;
use crate::error::SearchResult;

/// Port for a search index
#[async_trait]
pub trait SearchIndex: Send + Sync {
    /// Index a document
    async fn index(&self, document: &ArtifactSearchDocument) -> SearchResult<()>;

    /// Search for documents
    async fn search(&self, query: &str, repository_filter: Option<String>) -> SearchResult<Vec<ArtifactSearchDocument>>;
}

/// Port for an advanced search index
#[async_trait]
pub trait AdvancedSearchIndex: Send + Sync {
    /// Advanced search for documents with filters, facets, and sorting
    async fn advanced_search(&self, query: &crate::features::advanced_search::AdvancedSearchQuery) -> SearchResult<crate::features::advanced_search::AdvancedSearchResult>;
}

/// Port for index management operations
#[async_trait]
pub trait IndexManagement: Send + Sync {
    /// Create a new search index
    async fn create_index(&self, request: &crate::features::index_management::IndexCreationRequest) -> SearchResult<crate::features::index_management::IndexStatus>;

    /// Get the status of a search index
    async fn get_index_status(&self, index_name: &str) -> SearchResult<crate::features::index_management::IndexStatus>;

    /// Reindex documents from one index to another
    async fn reindex(&self, request: &crate::features::index_management::ReindexRequest) -> SearchResult<()>;
}
