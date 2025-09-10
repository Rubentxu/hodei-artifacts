use serde::{Deserialize, Serialize};
use crate::features::basic_search::dto::ArtifactDocument;

/// Query for full-text search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTextSearchQuery {
    pub q: String,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub language: Option<String>,
    pub fields: Option<Vec<String>>,
}

/// Results from full-text search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTextSearchResults {
    pub artifacts: Vec<ScoredArtifact>,
    pub total_count: usize,
    pub page: usize,
    pub page_size: usize,
    pub total_pages: usize,
    pub query_time_ms: u128,
    pub max_score: f32,
}

/// Artifact with relevance score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredArtifact {
    pub artifact: ArtifactDocument,
    pub score: f32,
    pub highlights: Vec<Highlight>,
}

/// Highlighted text snippet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Highlight {
    pub field: String,
    pub snippets: Vec<String>,
}

/// Indexed artifact representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedArtifact {
    pub id: String,
    pub content: String,
    pub metadata: ArtifactMetadata,
    pub language: String,
    pub indexed_at: chrono::DateTime<chrono::Utc>,
}

/// Artifact metadata for indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactMetadata {
    pub name: String,
    pub version: String,
    pub description: String,
    pub package_type: String,
    pub repository: String,
    pub tags: Vec<String>,
    pub authors: Vec<String>,
    pub licenses: Vec<String>,
    pub keywords: Vec<String>,
}

impl FullTextSearchResults {
    pub fn new(
        artifacts: Vec<ScoredArtifact>,
        total_count: usize,
        page: usize,
        page_size: usize,
        query_time_ms: u128,
        max_score: f32,
    ) -> Self {
        let total_pages = if page_size > 0 {
            total_count.div_ceil(page_size)
        } else {
            0
        };

        Self {
            artifacts,
            total_count,
            page,
            page_size,
            total_pages,
            query_time_ms,
            max_score,
        }
    }
}