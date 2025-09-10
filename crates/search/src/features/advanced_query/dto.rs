use serde::{Deserialize, Serialize};
use crate::features::basic_search::dto::ArtifactDocument;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedSearchQuery {
    pub q: String,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
    pub language: Option<String>,
    pub fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedQueryInfo {
    pub original_query: String,
    pub parsed_fields: Vec<String>,
    pub boolean_operators: Vec<String>,
    pub has_wildcards: bool,
    pub has_fuzzy: bool,
    pub has_ranges: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedSearchResults {
    pub artifacts: Vec<ArtifactDocument>,
    pub total_count: usize,
    pub page: usize,
    pub page_size: usize,
    pub total_pages: usize,
    pub query_parsed: ParsedQueryInfo,
    pub query_time_ms: u128,
}

impl AdvancedSearchResults {
    pub fn new(
        artifacts: Vec<ArtifactDocument>,
        total_count: usize,
        page: usize,
        page_size: usize,
        query_parsed: ParsedQueryInfo,
    ) -> Self {
        let total_pages = if page_size > 0 {
            (total_count + page_size - 1) / page_size
        } else {
            0
        };

        Self {
            artifacts,
            total_count,
            page,
            page_size,
            total_pages,
            query_parsed,
            query_time_ms: 0, // Will be set by the caller
        }
    }
    
    pub fn with_query_time(mut self, query_time_ms: u128) -> Self {
        self.query_time_ms = query_time_ms;
        self
    }
}