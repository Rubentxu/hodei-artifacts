use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, info, error};

use crate::features::basic_search::{
    dto::{ArtifactDocument},
    error::BasicSearchError,
    ports::ArtifactRepositoryPort,
};

/// Production adapter for artifact repository
/// This is a simplified in-memory implementation
/// In a real implementation, this would connect to MongoDB or another database
pub struct InMemoryArtifactRepositoryAdapter {
    pub artifacts: Arc<RwLock<HashMap<String, ArtifactDocument>>>,
}

impl InMemoryArtifactRepositoryAdapter {
    pub fn new() -> Self {
        Self {
            artifacts: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn add_test_artifact(&self, artifact: ArtifactDocument) {
        let mut artifacts = self.artifacts.write().unwrap();
        artifacts.insert(artifact.id.clone(), artifact);
    }
}

#[async_trait]
impl ArtifactRepositoryPort for InMemoryArtifactRepositoryAdapter {
    async fn get_artifact_by_id(
        &self,
        id: &str,
    ) -> Result<Option<ArtifactDocument>, BasicSearchError> {
        debug!(artifact_id = %id, "Getting artifact by ID from in-memory repository");
        
        let artifacts = self.artifacts.read().unwrap();
        Ok(artifacts.get(id).cloned())
    }
    
    async fn list_all_artifacts(
        &self,
        page: usize,
        page_size: usize,
    ) -> Result<(Vec<ArtifactDocument>, usize), BasicSearchError> {
        debug!(page = page, page_size = page_size, "Listing all artifacts from in-memory repository");
        
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
        
        info!(artifact_count = paginated.len(), total_count = total_count, "Artifacts listed successfully");
        Ok((paginated, total_count))
    }
}