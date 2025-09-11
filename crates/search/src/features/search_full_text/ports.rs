//! Segregated Ports for Full Text Search Feature
//!
//! This module defines specific interfaces following the Interface Segregation Principle (ISP)
//! for full-text search operations. Each port has a single, well-defined responsibility.

use async_trait::async_trait;
use std::sync::Arc;
use dto::*;
use error::*;

/// Port for full-text search operations
/// 
/// This port is responsible for executing search queries, retrieving results,
/// and providing search-related functionality like suggestions and facets.
#[async_trait]
pub trait FullTextSearchPort: Send + Sync {
    /// Execute a full-text search query
    async fn search(&self, query: FullTextSearchQuery) -> Result<FullTextSearchResults, SearchError>;
    
    /// Get search suggestions for a partial query
    async fn get_suggestions(&self, query: SearchSuggestionsQuery) -> Result<SearchSuggestionsResponse, SuggestionError>;
    
    /// Get search facets/aggregations
    async fn get_facets(&self, query: FullTextSearchQuery) -> Result<SearchFacets, FacetError>;
    
    /// Execute a more like this query
    async fn more_like_this(&self, document_id: &str, limit: usize) -> Result<FullTextSearchResults, SearchError>;
    
    /// Execute a search with scroll functionality for large result sets
    async fn search_with_scroll(&self, query: FullTextSearchQuery) -> Result<ScrollSearchResponse, SearchError>;
    
    /// Continue scrolling through results
    async fn continue_scroll(&self, scroll_id: &str) -> Result<ScrollSearchResponse, SearchError>;
}

/// Port for query analysis and optimization
/// 
/// This port handles query parsing, analysis, optimization, and performance monitoring.
#[async_trait]
pub trait QueryAnalyzerPort: Send + Sync {
    /// Analyze a query for performance and optimization opportunities
    async fn analyze_query(&self, command: AnalyzeQueryPerformanceCommand) -> Result<QueryPerformanceAnalysis, AnalysisError>;
    
    /// Parse and validate a search query
    async fn parse_query(&self, query: &str, mode: SearchMode) -> Result<ParsedQuery, ParseError>;
    
    /// Optimize a query for better performance
    async fn optimize_query(&self, parsed_query: ParsedQuery) -> Result<OptimizedQuery, OptimizationError>;
    
    /// Extract terms from a query with analysis
    async fn extract_query_terms(&self, query: &str, language: Option<&str>) -> Result<QueryTerms, ExtractionError>;
    
    /// Rewrite a query for better results (e.g., stemming, synonym expansion)
    async fn rewrite_query(&self, parsed_query: ParsedQuery) -> Result<RewrittenQuery, RewriteError>;
    
    /// Calculate query complexity score
    fn calculate_complexity(&self, parsed_query: &ParsedQuery, query_terms: &QueryTerms) -> f32;
    
    /// Generate query optimizations
    fn generate_optimizations(&self, parsed_query: &ParsedQuery, query_terms: &QueryTerms) -> Vec<QueryOptimization>;
    
    /// Classify query type
    fn classify_query_type(&self, parsed_query: &ParsedQuery) -> QueryType;
}

/// Port for relevance scoring and ranking
/// 
/// This port handles scoring algorithms, ranking functions, and relevance calculations.
#[async_trait]
pub trait RelevanceScorerPort: Send + Sync {
    /// Calculate relevance score for a document
    async fn calculate_score(&self, request: ScoreCalculationRequest) -> Result<RelevanceScore, ScoreError>;
    
    /// Calculate BM25 score
    async fn calculate_bm25_score(&self, request: BM25Request) -> Result<f32, ScoreError>;
    
    /// Calculate TF-IDF score
    async fn calculate_tfidf_score(&self, request: TFIDFRequest) -> Result<f32, ScoreError>;
    
    /// Combine multiple scores with weights
    async fn combine_scores(&self, scores: Vec<ScoreComponent>) -> Result<CombinedScore, ScoreError>;
    
    /// Normalize scores across result set
    async fn normalize_scores(&self, scores: Vec<RawScore>) -> Result<Vec<NormalizedScore>, ScoreError>;
    
    /// Rank documents by relevance
    async fn rank_documents(&self, documents: Vec<DocumentToRank>) -> Result<Vec<RankedDocument>, RankingError>;
}

/// Port for search result highlighting and snippet generation
/// 
/// This port handles the generation of highlighted text fragments and relevant snippets.
#[async_trait]
pub trait HighlighterPort: Send + Sync {
    /// Generate highlights for search results
    async fn generate_highlights(&self, request: HighlightRequest) -> Result<Vec<Highlight>, HighlightError>;
    
    /// Generate text snippets
    async fn generate_snippets(&self, request: SnippetRequest) -> Result<Vec<TextSnippet>, SnippetError>;
    
    /// Extract best passages from documents
    async fn extract_best_passages(&self, request: PassageExtractionRequest) -> Result<Vec<TextPassage>, PassageError>;
    
    /// Highlight terms in text
    async fn highlight_terms(&self, text: &str, terms: &[String], language: Option<&str>) -> Result<String, HighlightError>;
}

/// Port for search performance monitoring
/// 
/// This port provides monitoring capabilities for search operations,
/// including performance metrics and query statistics.
#[async_trait]
pub trait SearchPerformanceMonitorPort: Send + Sync {
    /// Record query performance metrics
    async fn record_query_metrics(&self, metrics: QueryMetrics) -> Result<(), MonitoringError>;
    
    /// Get search performance statistics
    async fn get_search_stats(&self, time_range: TimeRange) -> Result<SearchPerformanceStats, StatsError>;
    
    /// Get slow query analysis
    async fn get_slow_queries(&self, threshold_ms: u64, limit: usize) -> Result<Vec<SlowQueryInfo>, QueryError>;
    
    /// Monitor search index health
    async fn monitor_search_health(&self) -> Result<SearchHealthStatus, HealthError>;
    
    /// Get query patterns and trends
    async fn get_query_patterns(&self, time_range: TimeRange) -> Result<QueryPatterns, PatternError>;
}

/// Port for search index management
/// 
/// This port handles index operations like optimization, maintenance, and configuration.
#[async_trait]
pub trait SearchIndexManagerPort: Send + Sync {
    /// Get index statistics
    async fn get_index_stats(&self) -> Result<IndexStats, IndexError>;
    
    /// Optimize the search index
    async fn optimize_index(&self) -> Result<OptimizationResult, OptimizationError>;
    
    /// Rebuild the search index
    async fn rebuild_index(&self) -> Result<RebuildResult, RebuildError>;
    
    /// Clear the search index
    async fn clear_index(&self) -> Result<ClearResult, ClearError>;
    
    /// Validate the search index
    async fn validate_index(&self) -> Result<ValidationResult, ValidationError>;
    
    /// Get index configuration
    async fn get_index_config(&self) -> Result<IndexConfig, ConfigError>;
    
    /// Update index configuration
    async fn update_index_config(&self, config: IndexConfig) -> Result<ConfigUpdateResult, ConfigError>;
    
    /// Perform index maintenance tasks
    async fn perform_maintenance(&self, tasks: Vec<MaintenanceTask>) -> Result<MaintenanceResult, MaintenanceError>;
    
    /// Get index segments information
    async fn get_segments_info(&self) -> Result<Vec<SegmentInfo>, SegmentError>;
    
    /// Merge index segments
    async fn merge_segments(&self, merge_policy: MergePolicy) -> Result<MergeResult, MergeError>;
}

/// Error types for search operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum SearchError {
    #[error("Query parsing failed: {0}")]
    QueryParseFailed(String),
    
    #[error("Query execution failed: {0}")]
    QueryExecutionFailed(String),
    
    #[error("Index not found: {0}")]
    IndexNotFound(String),
    
    #[error("Index unavailable: {0}")]
    IndexUnavailable(String),
    
    #[error("Invalid query parameters: {0}")]
    InvalidQuery(String),
    
    #[error("Search timeout: {0}ms")]
    Timeout(u64),
    
    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),
    
    #[error("Internal search error: {0}")]
    InternalError(String),
}

/// Error types for suggestions
#[derive(Debug, Clone, thiserror::Error)]
pub enum SuggestionError {
    #[error("Failed to generate suggestions: {0}")]
    SuggestionGenerationFailed(String),
    
    #[error("Invalid suggestion query: {0}")]
    InvalidSuggestionQuery(String),
    
    #[error("Suggestion service unavailable")]
    ServiceUnavailable,
}

/// Error types for facets
#[derive(Debug, Clone, thiserror::Error)]
pub enum FacetError {
    #[error("Failed to generate facets: {0}")]
    FacetGenerationFailed(String),
    
    #[error("Invalid facet configuration: {0}")]
    InvalidFacetConfig(String),
    
    #[error("Facet calculation timeout")]
    Timeout,
}

/// Error types for query analysis
#[derive(Debug, Clone, thiserror::Error)]
pub enum AnalysisError {
    #[error("Query analysis failed: {0}")]
    AnalysisFailed(String),
    
    #[error("Performance analysis failed: {0}")]
    PerformanceAnalysisFailed(String),
    
    #[error("Query complexity analysis failed")]
    ComplexityAnalysisFailed,
}

/// Error types for query parsing
#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseError {
    #[error("Query parsing failed: {0}")]
    ParseFailed(String),
    
    #[error("Invalid query syntax: {0}")]
    InvalidSyntax(String),
    
    #[error("Unsupported query feature: {0}")]
    UnsupportedFeature(String),
}

/// Error types for query optimization
#[derive(Debug, Clone, thiserror::Error)]
pub enum OptimizationError {
    #[error("Query optimization failed: {0}")]
    OptimizationFailed(String),
    
    #[error("Cannot optimize query: {0}")]
    CannotOptimize(String),
}

/// Error types for term extraction
#[derive(Debug, Clone, thiserror::Error)]
pub enum ExtractionError {
    #[error("Term extraction failed: {0}")]
    ExtractionFailed(String),
    
    #[error("Language detection failed: {0}")]
    LanguageDetectionFailed(String),
}

/// Error types for query rewriting
#[derive(Debug, Clone, thiserror::Error)]
pub enum RewriteError {
    #[error("Query rewriting failed: {0}")]
    RewriteFailed(String),
    
    #[error("Synonym expansion failed: {0}")]
    SynonymExpansionFailed(String),
}

/// Error types for scoring
#[derive(Debug, Clone, thiserror::Error)]
pub enum ScoreError {
    #[error("Score calculation failed: {0}")]
    ScoreCalculationFailed(String),
    
    #[error("Invalid scoring parameters: {0}")]
    InvalidScoringParams(String),
    
    #[error("Scoring model not available: {0}")]
    ScoringModelNotAvailable(String),
}

/// Error types for ranking
#[derive(Debug, Clone, thiserror::Error)]
pub enum RankingError {
    #[error("Document ranking failed: {0}")]
    RankingFailed(String),
    
    #[error("Invalid ranking parameters: {0}")]
    InvalidRankingParams(String),
}

/// Error types for highlighting
#[derive(Debug, Clone, thiserror::Error)]
pub enum HighlightError {
    #[error("Highlight generation failed: {0}")]
    HighlightGenerationFailed(String),
    
    #[error("Invalid highlight parameters: {0}")]
    InvalidHighlightParams(String),
}

/// Error types for snippet generation
#[derive(Debug, Clone, thiserror::Error)]
pub enum SnippetError {
    #[error("Snippet generation failed: {0}")]
    SnippetGenerationFailed(String),
    
    #[error("Invalid snippet parameters: {0}")]
    InvalidSnippetParams(String),
}

/// Error types for passage extraction
#[derive(Debug, Clone, thiserror::Error)]
pub enum PassageError {
    #[error("Passage extraction failed: {0}")]
    PassageExtractionFailed(String),
    
    #[error("No relevant passages found")]
    NoPassagesFound,
}

/// Error types for monitoring
#[derive(Debug, Clone, thiserror::Error)]
pub enum MonitoringError {
    #[error("Failed to record metrics: {0}")]
    MetricsRecordingFailed(String),
    
    #[error("Metrics retrieval failed: {0}")]
    MetricsRetrievalFailed(String),
    
    #[error("Monitoring service unavailable")]
    ServiceUnavailable,
}

/// Error types for stats
#[derive(Debug, Clone, thiserror::Error)]
pub enum StatsError {
    #[error("Failed to get statistics: {0}")]
    StatsRetrievalFailed(String),
    
    #[error("Statistics not available")]
    StatsNotAvailable,
}

/// Error types for queries
#[derive(Debug, Clone, thiserror::Error)]
pub enum QueryError {
    #[error("Query analysis failed: {0}")]
    QueryAnalysisFailed(String),
    
    #[error("No slow queries found")]
    NoSlowQueriesFound,
}

/// Error types for health
#[derive(Debug, Clone, thiserror::Error)]
pub enum HealthError {
    #[error("Health check failed: {0}")]
    HealthCheckFailed(String),
    
    #[error("Search system unhealthy")]
    SystemUnhealthy,
}

/// Error types for patterns
#[derive(Debug, Clone, thiserror::Error)]
pub enum PatternError {
    #[error("Pattern analysis failed: {0}")]
    PatternAnalysisFailed(String),
    
    #[error("Insufficient data for pattern analysis")]
    InsufficientData,
}

/// Error types for index operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum IndexError {
    #[error("Index operation failed: {0}")]
    IndexOperationFailed(String),
    
    #[error("Index locked for maintenance")]
    IndexLocked,
    
    #[error("Index corruption detected")]
    IndexCorrupted,
    
    #[error("Index rebuild failed: {source}")]
    Rebuild {
        #[from]
        source: crate::features::search_full_text::error::RebuildError,
    },
    
    #[error("Index clear failed: {source}")]
    Clear {
        #[from]
        source: crate::features::search_full_text::error::ClearError,
    },
    
    #[error("Index validation failed: {source}")]
    Validate {
        #[from]
        source: crate::features::search_full_text::error::ValidationError,
    },
}

// Error types (MaintenanceError, SegmentError, MergeError) are defined in error.rs

/// Error types for configuration
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
    
    #[error("Configuration not found: {0}")]
    ConfigNotFound(String),
    
    #[error("Configuration update failed: {0}")]
    ConfigUpdateFailed(String),
}

// Request/Response types for various operations

#[derive(Debug, Clone)]
pub struct ScoreCalculationRequest {
    pub document_id: String,
    pub query_terms: Vec<String>,
    pub document_terms: Vec<String>,
    pub document_length: usize,
    pub avg_document_length: f32,
    pub scoring_params: ScoringParams,
}

#[derive(Debug, Clone)]
pub struct ScoringParams {
    pub k1: f32,
    pub b: f32,
    pub use_bm25: bool,
    pub use_tfidf: bool,
    pub custom_weights: std::collections::HashMap<String, f32>,
}

#[derive(Debug, Clone)]
pub struct BM25Request {
    pub term_frequency: f32,
    pub document_length: usize,
    pub avg_document_length: f32,
    pub total_documents: usize,
    pub document_frequency: usize,
    pub k1: f32,
    pub b: f32,
}

#[derive(Debug, Clone)]
pub struct TFIDFRequest {
    pub term_frequency: f32,
    pub document_frequency: usize,
    pub total_documents: usize,
}

#[derive(Debug, Clone)]
pub struct ScoreComponent {
    pub score_type: String,
    pub score: f32,
    pub weight: f32,
}

#[derive(Debug, Clone)]
pub struct RawScore {
    pub document_id: String,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct NormalizedScore {
    pub document_id: String,
    pub original_score: f32,
    pub normalized_score: f32,
}

#[derive(Debug, Clone)]
pub struct DocumentToRank {
    pub document_id: String,
    pub raw_score: f32,
    pub metadata: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct RankedDocument {
    pub document_id: String,
    pub rank: usize,
    pub final_score: f32,
    pub confidence: f32,
}

#[derive(Debug, Clone)]
pub struct RelevanceScore {
    pub document_id: String,
    pub score: f32,
    pub confidence: f32,
    pub score_components: Vec<ScoreComponent>,
}

#[derive(Debug, Clone)]
pub struct CombinedScore {
    pub document_id: String,
    pub final_score: f32,
    pub component_scores: Vec<ScoreComponent>,
}

#[derive(Debug, Clone)]
pub struct HighlightRequest {
    pub document_id: String,
    pub query_terms: Vec<String>,
    pub fields: Vec<String>,
    pub max_fragments: usize,
    pub fragment_size: usize,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SnippetRequest {
    pub document_id: String,
    pub content: String,
    pub query_terms: Vec<String>,
    pub max_snippets: usize,
    pub snippet_length: usize,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PassageExtractionRequest {
    pub document_id: String,
    pub content: String,
    pub query_terms: Vec<String>,
    pub max_passages: usize,
    pub passage_length: usize,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TextPassage {
    pub text: String,
    pub start_pos: usize,
    pub end_pos: usize,
    pub score: f32,
    pub matched_terms: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScrollSearchResponse {
    pub results: Vec<SearchResult>,
    pub scroll_id: String,
    pub total_hits: usize,
    pub query_time_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ParsedQuery {
    pub original_query: String,
    pub parsed_terms: Vec<QueryTerm>,
    pub operators: Vec<QueryOperator>,
    pub filters: Vec<QueryFilter>,
    pub query_type: QueryType,
}

#[derive(Debug, Clone)]
pub struct QueryTerm {
    pub term: String,
    pub field: Option<String>,
    pub boost: Option<f32>,
    pub fuzzy: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct QueryOperator {
    pub operator_type: OperatorType,
    pub operands: Vec<ParsedQuery>,
}

#[derive(Debug, Clone)]
pub struct QueryFilter {
    pub field: String,
    pub operator: FilterOperator,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OperatorType {
    And,
    Or,
    Not,
    Boost(f32),
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilterOperator {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    GreaterThan,
    LessThan,
    GreaterThanOrEqual,
    LessThanOrEqual,
    In,
    NotIn,
}

#[derive(Debug, Clone)]
pub struct OptimizedQuery {
    pub original_query: ParsedQuery,
    pub optimized_terms: Vec<QueryTerm>,
    pub optimization_hints: Vec<OptimizationHint>,
    pub estimated_cost: f32,
}

#[derive(Debug, Clone)]
pub struct OptimizationHint {
    pub hint_type: String,
    pub description: String,
    pub impact: String,
}

#[derive(Debug, Clone)]
pub struct QueryTerms {
    pub original_terms: Vec<String>,
    pub normalized_terms: Vec<String>,
    pub stop_words: Vec<String>,
    pub synonyms: Vec<String>,
    pub stemmed_terms: Vec<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RewrittenQuery {
    pub original_query: ParsedQuery,
    pub rewritten_terms: Vec<QueryTerm>,
    pub expansions: Vec<QueryExpansion>,
    pub rewrite_rules: Vec<RewriteRule>,
}

#[derive(Debug, Clone)]
pub struct QueryExpansion {
    pub expansion_type: String,
    pub original_term: String,
    pub expanded_terms: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RewriteRule {
    pub rule_type: String,
    pub description: String,
    pub applied_to: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct QueryMetrics {
    pub query_text: String,
    pub execution_time_ms: u64,
    pub documents_scanned: usize,
    pub documents_returned: usize,
    pub cache_hit: bool,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct SearchPerformanceStats {
    pub total_queries: usize,
    pub average_query_time_ms: f64,
    pub p95_query_time_ms: f64,
    pub p99_query_time_ms: f64,
    pub cache_hit_rate: f64,
    pub error_rate: f64,
    pub most_popular_queries: Vec<PopularQuery>,
    pub slowest_queries: Vec<SlowQueryInfo>,
}

#[derive(Debug, Clone)]
pub struct PopularQuery {
    pub query: String,
    pub count: usize,
    pub average_time_ms: f64,
}

#[derive(Debug, Clone)]
pub struct SlowQueryInfo {
    pub query: String,
    pub execution_time_ms: u64,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub user_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SearchHealthStatus {
    pub overall_status: HealthStatus,
    pub components: Vec<ComponentHealth>,
    pub last_check: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HealthStatus {
    Healthy,
    Warning,
    Unhealthy,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ComponentHealth {
    pub component_name: String,
    pub status: HealthStatus,
    pub message: Option<String>,
    pub metrics: Option<std::collections::HashMap<String, f32>>,
}

#[derive(Debug, Clone)]
pub struct QueryPatterns {
    pub popular_terms: Vec<TermFrequency>,
    pub query_length_distribution: std::collections::HashMap<usize, usize>,
    pub search_mode_distribution: std::collections::HashMap<SearchMode, usize>,
    pub temporal_patterns: Vec<TemporalPattern>,
}

#[derive(Debug, Clone)]
pub struct TermFrequency {
    pub term: String,
    pub frequency: usize,
    pub percentage: f32,
}

#[derive(Debug, Clone)]
pub struct TemporalPattern {
    pub time_period: String,
    pub query_count: usize,
    pub average_complexity: f32,
}

#[derive(Debug, Clone)]
pub struct TimeRange {
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct OptimizationResult {
    pub segments_before: usize,
    pub segments_after: usize,
    pub size_reduction_bytes: i64,
    pub time_taken_ms: u64,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct IndexConfig {
    pub index_name: String,
    pub settings: IndexSettings,
    pub analyzers: Vec<AnalyzerConfig>,
    pub similarity: SimilarityConfig,
}

#[derive(Debug, Clone)]
pub struct IndexSettings {
    pub number_of_shards: u32,
    pub number_of_replicas: u32,
    pub refresh_interval: String,
    pub max_result_window: usize,
}

#[derive(Debug, Clone)]
pub struct AnalyzerConfig {
    pub name: String,
    pub tokenizer: String,
    pub filters: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SimilarityConfig {
    pub algorithm: String,
    pub parameters: std::collections::HashMap<String, f32>,
}

#[derive(Debug, Clone)]
pub struct ConfigUpdateResult {
    pub success: bool,
    pub message: String,
    pub changes_applied: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MaintenanceTask {
    pub task_type: String,
    pub parameters: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct MaintenanceResult {
    pub tasks_completed: usize,
    pub tasks_failed: usize,
    pub time_taken_ms: u64,
    pub details: Vec<TaskResult>,
}

#[derive(Debug, Clone)]
pub struct TaskResult {
    pub task_type: String,
    pub success: bool,
    pub message: String,
    pub time_taken_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SegmentInfo {
    pub segment_id: String,
    pub document_count: usize,
    pub size_bytes: usize,
    pub deleted_documents: usize,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct MergePolicy {
    pub policy_type: String,
    pub max_segments: u32,
    pub max_segment_size_mb: u32,
}

#[derive(Debug, Clone)]
pub struct MergeResult {
    pub segments_merged: usize,
    pub segments_created: usize,
    pub size_reduction_bytes: i64,
    pub time_taken_ms: u64,
}

#[derive(Debug, Clone)]
pub struct RebuildResult {
    pub success: bool,
    pub documents_processed: usize,
    pub rebuild_time_ms: u64,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct ClearResult {
    pub success: bool,
    pub documents_removed: usize,
    pub clear_time_ms: u64,
}

#[derive(Debug, Clone)]
pub struct ValidationResult {
    pub is_valid: bool,
    pub issues: Vec<String>,
    pub validation_time_ms: u64,
}