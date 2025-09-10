use serde::{Deserialize, Serialize};
use crate::features::basic_search::dto::{ArtifactDocument, SearchResults};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedSearchQuery {
    pub q: String,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedSearchResults {
    pub artifacts: Vec<ArtifactDocument>,
    pub total_count: usize,
    pub page: usize,
    pub page_size: usize,
    pub total_pages: usize,
    pub query_parsed: ParsedQueryInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedQueryInfo {
    pub original_query: String,
    pub parsed_fields: Vec<FieldQuery>,
    pub boolean_operators: Vec<BooleanOperator>,
    pub has_wildcards: bool,
    pub has_fuzzy: bool,
    pub has_ranges: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldQuery {
    pub field: String,
    pub value: String,
    pub operator: QueryOperator,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryOperator {
    Equals,
    Contains,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    InRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BooleanOperator {
    And,
    Or,
    Not,
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
            query_parsed,
        }
    }
}