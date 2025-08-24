use async_trait::async_trait;
use crate::domain::model::ArtifactSearchDocument;
use crate::error::SearchResult;

/// Port for a search index
#[async_trait]
pub trait SearchIndex: Send + Sync {
    /// Index a document
    async fn index(&self, document: &ArtifactSearchDocument) -> SearchResult<()>;

    /// Search for documents
    async fn search(&self, query: &str) -> SearchResult<Vec<ArtifactSearchDocument>>;
}
