//! Search query domain models
//!
//! This module contains query-related domain entities and value objects
//! for constructing and parsing search queries.

use super::{IndexId, DocumentField};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Search query with filters and pagination
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    pub index_ids: Vec<IndexId>,
    pub filters: QueryFilters,
    pub pagination: Pagination,
    pub sorting: Sorting,
    pub options: SearchOptions,
}

impl SearchQuery {
    pub fn new(query: String) -> Self {
        Self {
            query,
            index_ids: Vec::new(),
            filters: QueryFilters::default(),
            pagination: Pagination::default(),
            sorting: Sorting::default(),
            options: SearchOptions::default(),
        }
    }
    
    pub fn with_indices(mut self, indices: Vec<IndexId>) -> Self {
        self.index_ids = indices;
        self
    }
    
    pub fn with_filters(mut self, filters: QueryFilters) -> Self {
        self.filters = filters;
        self
    }
    
    pub fn with_pagination(mut self, pagination: Pagination) -> Self {
        self.pagination = pagination;
        self
    }
    
    pub fn with_sorting(mut self, sorting: Sorting) -> Self {
        self.sorting = sorting;
        self
    }
    
    pub fn with_options(mut self, options: SearchOptions) -> Self {
        self.options = options;
        self
    }
    
    /// Check if query is empty
    pub fn is_empty(&self) -> bool {
        self.query.trim().is_empty()
    }
    
    /// Get query terms (simple split by whitespace)
    pub fn terms(&self) -> Vec<String> {
        self.query
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    }
    
    /// Calculate maximum number of results to return
    pub fn max_results(&self) -> usize {
        self.pagination.page_size
    }
    
    /// Calculate offset for pagination
    pub fn offset(&self) -> usize {
        self.pagination.page.saturating_sub(1) * self.pagination.page_size
    }
}

/// Query filters
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct QueryFilters {
    pub field_filters: HashMap<DocumentField, FieldFilter>,
    pub date_range: Option<DateRange>,
    pub numeric_range: Option<NumericRange>,
    pub tag_filters: Vec<String>,
    pub exclude_tags: Vec<String>,
}

impl QueryFilters {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn add_field_filter(&mut self, field: DocumentField, filter: FieldFilter) {
        self.field_filters.insert(field, filter);
    }
    
    pub fn set_date_range(&mut self, range: DateRange) {
        self.date_range = Some(range);
    }
    
    pub fn set_numeric_range(&mut self, range: NumericRange) {
        self.numeric_range = Some(range);
    }
    
    pub fn add_tag_filter(&mut self, tag: String) {
        self.tag_filters.push(tag);
    }
    
    pub fn add_exclude_tag(&mut self, tag: String) {
        self.exclude_tags.push(tag);
    }
    
    pub fn has_filters(&self) -> bool {
        !self.field_filters.is_empty() 
            || self.date_range.is_some() 
            || self.numeric_range.is_some()
            || !self.tag_filters.is_empty()
            || !self.exclude_tags.is_empty()
    }
}

/// Field filter for specific document fields
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldFilter {
    Exact(String),
    Contains(String),
    StartsWith(String),
    EndsWith(String),
    In(Vec<String>),
    NotIn(Vec<String>),
    Range(String, String),
}

/// Date range filter
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DateRange {
    pub start: Option<chrono::DateTime<chrono::Utc>>,
    pub end: Option<chrono::DateTime<chrono::Utc>>,
}

impl DateRange {
    pub fn new(
        start: Option<chrono::DateTime<chrono::Utc>>,
        end: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Self {
        Self { start, end }
    }
    
    pub fn from_start(start: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            start: Some(start),
            end: None,
        }
    }
    
    pub fn from_end(end: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            start: None,
            end: Some(end),
        }
    }
    
    pub fn between(start: chrono::DateTime<chrono::Utc>, end: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            start: Some(start),
            end: Some(end),
        }
    }
    
    pub fn contains(&self, date: chrono::DateTime<chrono::Utc>) -> bool {
        match (&self.start, &self.end) {
            (Some(start), Some(end)) => date >= *start && date <= *end,
            (Some(start), None) => date >= *start,
            (None, Some(end)) => date <= *end,
            (None, None) => true,
        }
    }
}

/// Numeric range filter
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NumericRange {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

impl NumericRange {
    pub fn new(min: Option<f64>, max: Option<f64>) -> Self {
        Self { min, max }
    }
    
    pub fn from_min(min: f64) -> Self {
        Self {
            min: Some(min),
            max: None,
        }
    }
    
    pub fn from_max(max: f64) -> Self {
        Self {
            min: None,
            max: Some(max),
        }
    }
    
    pub fn between(min: f64, max: f64) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
        }
    }
    
    pub fn contains(&self, value: f64) -> bool {
        match (&self.min, &self.max) {
            (Some(min), Some(max)) => value >= *min && value <= *max,
            (Some(min), None) => value >= *min,
            (None, Some(max)) => value <= *max,
            (None, None) => true,
        }
    }
}

/// Pagination configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pagination {
    pub page: usize,
    pub page_size: usize,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            page: 1,
            page_size: 20,
        }
    }
}

impl Pagination {
    pub fn new(page: usize, page_size: usize) -> Self {
        Self { page, page_size }
    }
    
    pub fn first() -> Self {
        Self {
            page: 1,
            page_size: 20,
        }
    }
    
    pub fn with_size(self, page_size: usize) -> Self {
        Self { page_size, ..self }
    }
    
    /// Calculate the offset for database queries
    pub fn offset(&self) -> usize {
        self.page.saturating_sub(1) * self.page_size
    }
    
    /// Calculate the limit for database queries
    pub fn limit(&self) -> usize {
        self.page_size
    }
    
    /// Check if pagination is valid
    pub fn is_valid(&self) -> bool {
        self.page > 0 && self.page_size > 0 && self.page_size <= 1000
    }
}

/// Sorting configuration
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Sorting {
    pub sort_by: Vec<SortField>,
}

impl Sorting {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn add_sort_field(&mut self, field: SortField) {
        self.sort_by.push(field);
    }
    
    pub fn relevance() -> Self {
        Self {
            sort_by: vec![SortField::relevance()],
        }
    }
    
    pub fn newest_first() -> Self {
        Self {
            sort_by: vec![SortField::new("created_at".to_string(), SortDirection::Descending)],
        }
    }
    
    pub fn oldest_first() -> Self {
        Self {
            sort_by: vec![SortField::new("created_at".to_string(), SortDirection::Ascending)],
        }
    }
    
    pub fn is_empty(&self) -> bool {
        self.sort_by.is_empty()
    }
}

/// Sort field definition
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SortField {
    pub field: String,
    pub direction: SortDirection,
}

impl SortField {
    pub fn new(field: String, direction: SortDirection) -> Self {
        Self { field, direction }
    }
    
    pub fn relevance() -> Self {
        Self {
            field: "_score".to_string(),
            direction: SortDirection::Descending,
        }
    }
    
    pub fn ascending(field: String) -> Self {
        Self {
            field,
            direction: SortDirection::Ascending,
        }
    }
    
    pub fn descending(field: String) -> Self {
        Self {
            field,
            direction: SortDirection::Descending,
        }
    }
}

/// Sort direction
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn as_str(&self) -> &str {
        match self {
            SortDirection::Ascending => "asc",
            SortDirection::Descending => "desc",
        }
    }
    
    pub fn reverse(&self) -> Self {
        match self {
            SortDirection::Ascending => SortDirection::Descending,
            SortDirection::Descending => SortDirection::Ascending,
        }
    }
}

/// Search options
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchOptions {
    pub fuzzy_search: bool,
    pub phrase_search: bool,
    pub include_snippets: bool,
    pub include_highlights: bool,
    pub explain_scoring: bool,
    pub timeout_ms: Option<u64>,
    pub min_score: Option<f64>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            fuzzy_search: true,
            phrase_search: false,
            include_snippets: true,
            include_highlights: true,
            explain_scoring: false,
            timeout_ms: None,
            min_score: None,
        }
    }
}

impl SearchOptions {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn with_fuzzy_search(mut self, enabled: bool) -> Self {
        self.fuzzy_search = enabled;
        self
    }
    
    pub fn with_phrase_search(mut self, enabled: bool) -> Self {
        self.phrase_search = enabled;
        self
    }
    
    pub fn with_snippets(mut self, enabled: bool) -> Self {
        self.include_snippets = enabled;
        self
    }
    
    pub fn with_highlights(mut self, enabled: bool) -> Self {
        self.include_highlights = enabled;
        self
    }
    
    pub fn with_explain_scoring(mut self, enabled: bool) -> Self {
        self.explain_scoring = enabled;
        self
    }
    
    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }
    
    pub fn with_min_score(mut self, min_score: f64) -> Self {
        self.min_score = Some(min_score);
        self
    }
}

/// Advanced search query with boolean operators
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdvancedSearchQuery {
    pub must: Vec<QueryClause>,
    pub should: Vec<QueryClause>,
    pub must_not: Vec<QueryClause>,
    pub filters: QueryFilters,
    pub pagination: Pagination,
    pub options: SearchOptions,
}

impl AdvancedSearchQuery {
    pub fn new() -> Self {
        Self {
            must: Vec::new(),
            should: Vec::new(),
            must_not: Vec::new(),
            filters: QueryFilters::default(),
            pagination: Pagination::default(),
            options: SearchOptions::default(),
        }
    }
    
    pub fn must(mut self, clause: QueryClause) -> Self {
        self.must.push(clause);
        self
    }
    
    pub fn should(mut self, clause: QueryClause) -> Self {
        self.should.push(clause);
        self
    }
    
    pub fn must_not(mut self, clause: QueryClause) -> Self {
        self.must_not.push(clause);
        self
    }
    
    pub fn with_filters(mut self, filters: QueryFilters) -> Self {
        self.filters = filters;
        self
    }
    
    pub fn minimum_should_match(&self) -> usize {
        if self.should.is_empty() {
            0
        } else {
            1 // At least one should clause must match
        }
    }
}

/// Query clause for advanced searches
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueryClause {
    Term { field: DocumentField, value: String },
    Phrase { field: DocumentField, phrase: String },
    Wildcard { field: DocumentField, pattern: String },
    Range { field: DocumentField, range: String },
    Exists { field: DocumentField },
    Fuzzy { field: DocumentField, value: String, fuzziness: u32 },
    Boost { clause: Box<QueryClause>, boost: f64 },
    Nested { path: String, query: Box<AdvancedSearchQuery> },
}