//! Data Transfer Objects for Full Text Search Feature
//!
//! This module contains all the DTOs for full-text search operations,
//! following VSA principles with segregated interfaces.

use serde::{Serialize, Deserialize};
use uuid::Uuid;
use std::collections::HashMap;

/// Query for full-text search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTextSearchQuery {
    /// The search query string
    pub q: String,
    /// Filter by artifact type
    pub artifact_type: Option<String>,
    /// Filter by language
    pub language: Option<String>,
    /// Filter by tags
    pub tags: Option<Vec<String>>,
    /// Filter by date range
    pub date_range: Option<DateRange>,
    /// Search mode (simple, phrase, boolean)
    pub search_mode: SearchMode,
    /// Page number (1-based)
    pub page: Option<usize>,
    /// Number of results per page
    pub page_size: Option<usize>,
    /// Whether to include highlights
    pub include_highlights: bool,
    /// Whether to include snippets
    pub include_snippets: bool,
    /// Maximum snippet length
    pub snippet_length: Option<usize>,
    /// Sort order
    pub sort_order: SortOrder,
    /// Minimum relevance score threshold
    pub min_score: Option<f32>,
    /// Fuzziness level for approximate matching
    pub fuzziness: Option<u32>,
    /// Whether to enable stemming
    pub enable_stemming: Option<bool>,
    /// Whether to enable phonetic matching
    pub enable_phonetic: Option<bool>,
}

/// Response for full-text search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullTextSearchResults {
    /// List of search results
    pub results: Vec<SearchResult>,
    /// Total number of results matching the query
    pub total_count: usize,
    /// Current page number
    pub page: usize,
    /// Number of results per page
    pub page_size: usize,
    /// Query execution time in milliseconds
    pub query_time_ms: u64,
    /// Maximum relevance score in results
    pub max_score: f32,
    /// Search metadata
    pub metadata: SearchMetadata,
    /// Facets if requested
    pub facets: Option<SearchFacets>,
    /// Suggestions for query refinement
    pub suggestions: Option<Vec<SearchSuggestion>>,
}

/// Individual search result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Document ID
    pub document_id: String,
    /// Artifact metadata
    pub metadata: ArtifactMetadata,
    /// Relevance score
    pub score: f32,
    /// Highlighted snippets
    pub highlights: Vec<Highlight>,
    /// Snippets from the document
    pub snippets: Vec<TextSnippet>,
    /// Search result ranking information
    pub ranking: RankingInfo,
    /// Language of the document
    pub language: Option<String>,
    /// When the document was indexed
    pub indexed_at: chrono::DateTime<chrono::Utc>,
}

/// Highlighted text fragment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Highlight {
    /// Field name containing the highlight
    pub field: String,
    /// Highlighted text fragment
    pub text: String,
    /// Position of the highlight in the original text
    pub position: Option<usize>,
    /// Confidence score for the highlight
    pub confidence: Option<f32>,
}

/// Text snippet from document
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextSnippet {
    /// The snippet text
    pub text: String,
    /// Field the snippet is from
    pub field: String,
    /// Starting position in the original text
    pub start_pos: usize,
    /// Ending position in the original text
    pub end_pos: usize,
    /// Relevance score for this snippet
    pub score: f32,
}

/// Ranking information for search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingInfo {
    /// BM25 score
    pub bm25_score: Option<f32>,
    /// TF-IDF score
    pub tfidf_score: Option<f32>,
    /// PageRank score (if applicable)
    pub pagerank_score: Option<f32>,
    /// Freshness score (based on recency)
    pub freshness_score: Option<f32>,
    /// Popularity score (if applicable)
    pub popularity_score: Option<f32>,
    /// Final combined score
    pub combined_score: f32,
}

/// Search metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMetadata {
    /// Query execution plan
    pub execution_plan: Option<String>,
    /// Number of terms in the query
    pub query_term_count: usize,
    /// Whether the query was rewritten
    pub query_rewritten: bool,
    /// Query expansion terms
    pub expansion_terms: Option<Vec<String>>,
    /// Search engine version
    pub engine_version: String,
    /// Index statistics at time of search
    pub index_stats: IndexStats,
}

/// Search facets/aggregations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchFacets {
    /// Artifact type distribution
    pub artifact_types: Vec<FacetCount>,
    /// Language distribution
    pub languages: Vec<FacetCount>,
    /// Tag distribution
    pub tags: Vec<FacetCount>,
    /// Date distribution
    pub date_range: Option<DateRangeFacet>,
}

/// Facet count
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacetCount {
    /// Facet value
    pub value: String,
    /// Count of documents with this value
    pub count: usize,
    /// Percentage of total results
    pub percentage: f32,
}

/// Date range facet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRangeFacet {
    /// Date buckets
    pub buckets: Vec<DateBucket>,
    /// Date range granularity
    pub granularity: DateGranularity,
}

/// Date bucket for faceting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateBucket {
    /// Start date of the bucket
    pub start_date: chrono::DateTime<chrono::Utc>,
    /// End date of the bucket
    pub end_date: chrono::DateTime<chrono::Utc>,
    /// Count of documents in this bucket
    pub count: usize,
}

/// Search suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSuggestion {
    /// Suggested query text
    pub text: String,
    /// Highlighted version of the suggestion
    pub highlighted: Option<String>,
    /// Confidence score for this suggestion
    pub score: f32,
    /// Type of suggestion
    pub suggestion_type: SuggestionType,
}

/// Date range filter
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRange {
    /// Start date
    pub start: chrono::DateTime<chrono::Utc>,
    /// End date
    pub end: chrono::DateTime<chrono::Utc>,
}

/// Search mode
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SearchMode {
    /// Simple keyword search
    Simple,
    /// Phrase search (exact match)
    Phrase,
    /// Boolean search (AND, OR, NOT)
    Boolean,
    /// Fuzzy search (approximate matching)
    Fuzzy,
    /// Semantic search (meaning-based)
    Semantic,
}

/// Sort order for results
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SortOrder {
    /// Sort by relevance score (default)
    Relevance,
    /// Sort by date (newest first)
    DateDesc,
    /// Sort by date (oldest first)
    DateAsc,
    /// Sort by document title
    TitleAsc,
    /// Sort by document title (descending)
    TitleDesc,
    /// Sort by popularity
    Popularity,
    /// Custom sort criteria
    Custom(String),
}

/// Suggestion type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SuggestionType {
    /// Spelling correction
    Spelling,
    /// Query expansion
    Expansion,
    /// Related terms
    Related,
    /// Autocomplete suggestion
    Autocomplete,
}

/// Date granularity for faceting
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DateGranularity {
    /// Year granularity
    Year,
    /// Month granularity
    Month,
    /// Day granularity
    Day,
    /// Hour granularity
    Hour,
}

/// Query for search suggestions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSuggestionsQuery {
    /// Partial query string
    pub partial_query: String,
    /// Maximum number of suggestions
    pub limit: Option<usize>,
    /// Types of suggestions to return
    pub suggestion_types: Option<Vec<SuggestionType>>,
    /// Context for suggestions (e.g., field name)
    pub context: Option<String>,
    /// Language for suggestions
    pub language: Option<String>,
}

/// Response for search suggestions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchSuggestionsResponse {
    /// List of suggestions
    pub suggestions: Vec<SearchSuggestion>,
    /// Query execution time in milliseconds
    pub query_time_ms: u64,
    /// Total suggestions available
    pub total_count: usize,
}

/// Command to analyze query performance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyzeQueryPerformanceCommand {
    /// The query to analyze
    pub query: FullTextSearchQuery,
    /// Whether to include detailed timing information
    pub include_timing: bool,
    /// Whether to include execution plan
    pub include_execution_plan: bool,
    /// Whether to include index statistics
    pub include_index_stats: bool,
}

/// Response for query performance analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPerformanceAnalysis {
    /// Query analysis results
    pub analysis: QueryAnalysis,
    /// Performance metrics
    pub metrics: QueryPerformanceMetrics,
    /// Execution plan
    pub execution_plan: Option<ExecutionPlan>,
    /// Index statistics
    pub index_stats: Option<IndexStats>,
}

/// Query analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryAnalysis {
    /// Query complexity score
    pub complexity_score: f32,
    /// Query terms analysis
    pub terms_analysis: QueryTermsAnalysis,
    /// Query type classification
    pub query_type: QueryType,
    /// Potential optimizations
    pub optimizations: Vec<QueryOptimization>,
}

/// Query terms analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryTermsAnalysis {
    /// Number of query terms
    pub term_count: usize,
    /// Unique terms
    pub unique_terms: Vec<String>,
    /// Stop words in query
    pub stop_words: Vec<String>,
    /// Rare terms in query
    pub rare_terms: Vec<String>,
    /// Common terms in query
    pub common_terms: Vec<String>,
}

/// Query type classification
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum QueryType {
    /// Simple keyword query
    SimpleKeyword,
    /// Phrase query
    Phrase,
    /// Boolean query
    Boolean,
    /// Complex query with multiple operators
    Complex,
    /// Fuzzy query
    Fuzzy,
    /// Range query
    Range,
    /// Prefix/wildcard query
    Prefix,
}

/// Query optimization suggestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryOptimization {
    /// Description of the optimization
    pub description: String,
    /// Expected improvement
    pub expected_improvement: String,
    /// Priority level
    pub priority: OptimizationPriority,
}

/// Optimization priority
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OptimizationPriority {
    /// Low priority
    Low,
    /// Medium priority
    Medium,
    /// High priority
    High,
    /// Critical priority
    Critical,
}

/// Query performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryPerformanceMetrics {
    /// Query parsing time in milliseconds
    pub parsing_time_ms: f32,
    /// Query optimization time in milliseconds
    pub optimization_time_ms: f32,
    /// Index scan time in milliseconds
    pub scan_time_ms: f32,
    /// Results processing time in milliseconds
    pub processing_time_ms: f32,
    /// Total query execution time in milliseconds
    pub total_time_ms: f32,
    /// Number of documents scanned
    pub documents_scanned: usize,
    /// Number of documents matched
    pub documents_matched: usize,
    /// Memory usage in bytes
    pub memory_usage_bytes: usize,
    /// CPU usage percentage
    pub cpu_usage_percent: f32,
}

/// Execution plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    /// Plan nodes
    pub nodes: Vec<ExecutionPlanNode>,
    /// Estimated cost
    pub estimated_cost: f32,
    /// Estimated rows
    pub estimated_rows: usize,
}

/// Execution plan node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlanNode {
    /// Node type
    pub node_type: String,
    /// Node description
    pub description: String,
    /// Node cost
    pub cost: f32,
    /// Estimated rows for this node
    pub estimated_rows: usize,
    /// Child nodes
    pub children: Vec<ExecutionPlanNode>,
}

/// Reuse artifact metadata from index_text_documents
pub use crate::features::index_text_documents::dto::ArtifactMetadata;

/// Reuse index stats from index_text_documents
pub use crate::features::index_text_documents::dto::IndexStats;

impl Default for FullTextSearchQuery {
    fn default() -> Self {
        Self {
            q: String::new(),
            artifact_type: None,
            language: None,
            tags: None,
            date_range: None,
            search_mode: SearchMode::Simple,
            page: Some(1),
            page_size: Some(20),
            include_highlights: true,
            include_snippets: true,
            snippet_length: Some(150),
            sort_order: SortOrder::Relevance,
            min_score: None,
            fuzziness: None,
            enable_stemming: Some(true),
            enable_phonetic: Some(false),
        }
    }
}

impl FullTextSearchQuery {
    /// Create a test query for unit tests
    pub fn test_data() -> Self {
        Self {
            q: "test query".to_string(),
            artifact_type: Some("jar".to_string()),
            language: Some("en".to_string()),
            tags: Some(vec!["test".to_string(), "sample".to_string()]),
            date_range: Some(DateRange {
                start: chrono::Utc::now() - chrono::Duration::days(30),
                end: chrono::Utc::now(),
            }),
            search_mode: SearchMode::Simple,
            page: Some(1),
            page_size: Some(10),
            include_highlights: true,
            include_snippets: true,
            snippet_length: Some(100),
            sort_order: SortOrder::Relevance,
            min_score: Some(0.1),
            fuzziness: Some(1),
            enable_stemming: Some(true),
            enable_phonetic: Some(false),
        }
    }
}

impl FullTextSearchResults {
    /// Create empty search results
    pub fn empty() -> Self {
        Self {
            results: Vec::new(),
            total_count: 0,
            page: 1,
            page_size: 20,
            query_time_ms: 0,
            max_score: 0.0,
            metadata: SearchMetadata {
                execution_plan: None,
                query_term_count: 0,
                query_rewritten: false,
                expansion_terms: None,
                engine_version: "1.0.0".to_string(),
                index_stats: IndexStats {
                    total_documents: 0,
                    total_terms: 0,
                    avg_terms_per_document: 0.0,
                    index_size_bytes: 0,
                    memory_usage_bytes: 0,
                    segment_count: 0,
                    created_at: chrono::Utc::now(),
                    last_optimized_at: None,
                },
            },
            facets: None,
            suggestions: None,
        }
    }
}