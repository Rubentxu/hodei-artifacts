use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::debug;

use crate::features::basic_search::{
    dto::{SearchQuery, SearchResults, ArtifactDocument},
    error::BasicSearchError,
    ports::SearchIndexPort,
    infrastructure::tantivy_index::TantivySearchIndex,
};

pub struct TantivySearchAdapter {
    index: Arc<TantivySearchIndex>,
}

// In-memory repository adapter moved here per SRP and architecture
// In a real implementation, this would connect to MongoDB or another database
#[derive(Debug, Clone)]
pub struct InMemoryArtifactRepositoryAdapter {
    pub artifacts: Arc<RwLock<HashMap<String, ArtifactDocument>>>,
}

impl InMemoryArtifactRepositoryAdapter {
    pub fn new() -> Self {
        Self {
            artifacts: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub fn add_artifact(&self, artifact: ArtifactDocument) {
        let mut artifacts = self.artifacts.write().unwrap();
        artifacts.insert(artifact.id.clone(), artifact);
    }
}

impl Default for InMemoryArtifactRepositoryAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl InMemoryArtifactRepositoryAdapter {
    pub async fn get_artifact_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ArtifactDocument>, BasicSearchError> {
        let artifacts = self.artifacts.read().unwrap();
        Ok(artifacts.get(id).cloned())
    }
    
    pub async fn list_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<(Vec<ArtifactDocument>, usize), BasicSearchError> {
        let artifacts = self.artifacts.read().unwrap();
        let all_artifacts: Vec<ArtifactDocument> = artifacts.values().cloned().collect();
        let total_count = all_artifacts.len();
        
        let offset = (page - 1) * page_size;
        let paginated = if offset < total_count {
            let end = std::cmp::min(offset + page_size, total_count);
            all_artifacts[offset..end].to_vec()
        } else {
            Vec::new()
        };
        
        Ok((paginated, total_count))
    }
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