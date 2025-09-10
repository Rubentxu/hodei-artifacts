use async_trait::async_trait;
use crate::features::basic_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::BasicSearchError,
};

#[async_trait]
pub trait SearchIndexPort: Send + Sync {
    async fn search(
        &self,
        query: &SearchQuery,
    ) -> Result<SearchResults, BasicSearchError>;
    
    async fn index_artifact(
        &self,
        artifact: &ArtifactDocument,
    ) -> Result<(), BasicSearchError>;
    
    async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<SearchResults, BasicSearchError>;
}

#[async_trait]
pub trait ArtifactRepositoryPort: Send + Sync {
    async fn get_artifact_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ArtifactDocument>, BasicSearchError>;
    
    async fn list_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<(Vec<ArtifactDocument>, usize), BasicSearchError>;
}

#[async_trait]
pub trait EventPublisherPort: Send + Sync {
    async fn publish_search_query_executed(
        &self,
        query: &str,
        result_count: usize,
    ) -> Result<(), BasicSearchError>;
    
    async fn publish_search_result_clicked(
        &self,
        artifact_id: &str,
    ) -> Result<(), BasicSearchError>;
}