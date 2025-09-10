use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub q: String,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub language: Option<String>,
    pub fields: Option<Vec<String>>,
}

impl fmt::Display for SearchQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.q)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactDocument {
    pub id: String,
    pub name: String,
    pub version: String,
    pub package_type: String,
    pub repository: String,
    pub description: String,
    pub content: String,
    pub score: f32,
}

impl ArtifactDocument {
    pub fn new(
        id: String,
        name: String,
        version: String,
        package_type: String,
        repository: String,
        description: String,
        content: String,
        score: f32,
    ) -> Self {
        Self {
            id,
            name,
            version,
            package_type,
            repository,
            description,
            content,
            score,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub artifacts: Vec<ArtifactDocument>,
    pub total_count: usize,
    pub page: usize,
    pub page_size: usize,
    pub total_pages: usize,
    pub query_time_ms: u128,
    pub max_score: f32,
}

impl SearchResults {
    pub fn new(artifacts: Vec<ArtifactDocument>, total_count: usize, page: usize, page_size: usize) -> Self {
        let total_pages = if page_size > 0 {
            (total_count + page_size - 1) / page_size
        } else {
            0
        };
        
        let max_score = artifacts.iter()
            .map(|a| a.score)
            .fold(0.0f32, |acc, x| if x > acc { x } else { acc });

        Self {
            artifacts,
            total_count,
            page,
            page_size,
            total_pages,
            query_time_ms: 0, // Will be set by the caller
            max_score,
        }
    }
    
    pub fn with_query_time(mut self, query_time_ms: u128) -> Self {
        self.query_time_ms = query_time_ms;
        self
    }
    
    pub fn with_max_score(mut self, max_score: f32) -> Self {
        self.max_score = max_score;
        self
    }
}