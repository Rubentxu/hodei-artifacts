use async_trait::async_trait;
use std::sync::Arc;
use tracing::debug;

use crate::features::basic_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::BasicSearchError,
    ports::SearchIndexPort,
    infrastructure::TantivySearchIndex,
};

pub struct TantivySearchAdapter {
    index: Arc<TantivySearchIndex>,
}

impl TantivySearchAdapter {
    pub fn new() -> Result<Self, BasicSearchError> {
        let index = TantivySearchIndex::new()?;
        Ok(Self {
            index: Arc::new(index),
        })
    }
}

#[async_trait]
impl SearchIndexPort for TantivySearchAdapter {
    async fn search(
        &self,
        query: &SearchQuery,
    ) -> Result<SearchResults, BasicSearchError> {
        debug!(query = %query.q, "Searching in Tantivy adapter");
        self.index.search(query).await
    }
    
    async fn index_artifact(
        &self,
        artifact: &ArtifactDocument,
    ) -> Result<(), BasicSearchError> {
        debug!(artifact_id = %artifact.id, "Indexing artifact in Tantivy adapter");
        self.index.index_artifact(artifact).await
    }
    
    async fn get_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<SearchResults, BasicSearchError> {
        debug!(page = page, page_size = page_size, "Getting all artifacts in Tantivy adapter");
        self.index.get_all_artifacts(page, page_size).await
    }
}