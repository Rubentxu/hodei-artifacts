use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::features::basic_search::{
    dto::ArtifactDocument,
    error::BasicSearchError,
};

// Production adapter for artifact repository
// This is a simplified in-memory implementation
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