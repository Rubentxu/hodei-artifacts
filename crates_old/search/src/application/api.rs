use std::sync::Arc;
use serde::{Deserialize, Serialize};

use artifact::application::ports::ArtifactRepository;
use iam::application::ports::Authorization;
use crate::error::SearchError;
use artifact::domain::model::Artifact; // Assuming Artifact is needed for search results

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchArtifactsQuery {
    pub query: Option<String>,
    pub ecosystem: Option<String>,
    pub group_id: Option<String>,
    pub artifact_id: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SearchResult {
    pub artifacts: Vec<Artifact>, // Assuming Artifact is the search result item
    pub total_count: u64,
}

pub struct SearchApi<R, A>
where
    R: ArtifactRepository,
    A: Authorization,
{
    artifact_repository: Arc<R>,
    authorization: Arc<A>,
}

impl<R, A> SearchApi<R, A>
where
    R: ArtifactRepository,
    A: Authorization,
{
    pub fn new(artifact_repository: Arc<R>, authorization: Arc<A>) -> Self {
        Self {
            artifact_repository,
            authorization,
        }
    }

    pub async fn search_artifacts(&self, query: SearchArtifactsQuery) -> Result<SearchResult, SearchError> {
        // TODO: Implement authorization check here

        // For now, a basic implementation that just lists all artifacts
        // and filters them based on the query. This will be replaced by
        // actual search index integration later.

        let all_artifacts = self.artifact_repository.find_all_artifacts().await?;

        let filtered_artifacts: Vec<Artifact> = all_artifacts.into_iter().filter(|artifact| {
            if let Some(q) = &query.query {
                // Basic text search across relevant fields
                if !(artifact.file_name.contains(q) ||
                     artifact.version.0.contains(q) ||
                     artifact.id.0.to_string().contains(q) ||
                     artifact.repository_id.0.to_string().contains(q)) {
                    return false;
                }
            }
            // TODO: Add more sophisticated filtering based on ecosystem, group_id, etc.
            true
        }).collect();

        Ok(SearchResult {
            artifacts: filtered_artifacts.clone(),
            total_count: filtered_artifacts.len() as u64,
        })
    }
}
