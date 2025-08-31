//! Basic search functionality for Search bounded context
//!
//! Implements simple text-based search following VSA principles
//! This is a vertical slice containing all logic for basic search

use crate::application::ports::SearchIndex;
use crate::domain::model::ArtifactSearchDocument;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

// DTOs for basic search feature
#[derive(Debug, Serialize, Deserialize)]
pub struct BasicSearchQuery {
    pub query: String,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    pub repository_filter: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BasicSearchResult {
    pub artifacts: Vec<ArtifactSearchResult>,
    pub total_count: u64,
    pub search_time_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArtifactSearchResult {
    pub artifact_id: String,
    pub name: String,
    pub version: String,
    pub repository: String,
    pub description: Option<String>,
    pub relevance_score: f64,
}

// Placeholder handler - will be implemented following VSA TDD approach
pub async fn handle_basic_search(
    search_index: Arc<dyn SearchIndex>,
    query: BasicSearchQuery,
) -> Result<BasicSearchResult, crate::error::SearchError> {
    let start_time = Instant::now();

    let search_results = search_index.search(&query.query, query.repository_filter).await?;

    let total_count = search_results.len() as u64;

    let artifacts = search_results
        .into_iter()
        .map(map_to_search_result)
        .collect();

    let search_time_ms = start_time.elapsed().as_millis() as u64;

    Ok(BasicSearchResult {
        artifacts,
        total_count,
        search_time_ms,
    })
}

fn map_to_search_result(doc: ArtifactSearchDocument) -> ArtifactSearchResult {
    ArtifactSearchResult {
        artifact_id: doc.artifact_id.to_string(),
        name: doc.name,
        version: doc.version,
        repository: doc.repository_id.to_string(),
        description: doc.description,
        relevance_score: doc.relevance_score,
    }
}
