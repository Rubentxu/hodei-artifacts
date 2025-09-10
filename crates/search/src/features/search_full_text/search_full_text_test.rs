//! Unit Tests for Full Text Search Feature
//!
//! Comprehensive test suite covering all components of the search feature
//! following TDD principles and testing best practices.

use std::sync::Arc;
use tokio_test;
use tracing_test::traced_test;

use super::*;
use super::dto::*;
use super::error::*;
use super::use_case::*;
use super::api::*;
use super::di::*;
use super::ports::*;

// Mock implementations for testing
pub struct MockFullTextSearchAdapter {
    pub should_fail: bool,
    pub search_results: Vec<SearchResult>,
}

impl MockFullTextSearchAdapter {
    pub fn new() -> Self {
        Self {
            should_fail: false,
            search_results: vec![],
        }
    }
    
    pub fn with_results(mut self, results: Vec<SearchResult>) -> Self {
        self.search_results = results;
        self
    }
    
    pub fn failing(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl FullTextSearchPort for MockFullTextSearchAdapter {
    async fn search(&self, query: FullTextSearchQuery) -> Result<FullTextSearchResults, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::Search {
                source: SearchError::QueryExecutionFailed("Mock search failed".to_string()),
            });
        }
        
        Ok(FullTextSearchResults {
            results: self.search_results.clone(),
            total_count: self.search_results.len(),
            page: query.page.unwrap_or(1),
            page_size: query.page_size.unwrap_or(20),
            query_time_ms: 10,
            max_score: 1.0,
            metadata: SearchMetadata::default(),
            facets: None,
            suggestions: None,
        })
    }
    
    async fn get_suggestions(&self, query: SearchSuggestionsQuery) -> Result<SearchSuggestionsResponse, FullTextSearchError> {
        Ok(SearchSuggestionsResponse {
            suggestions: vec![],
            query_time_ms: 5,
            total_count: 0,
        })
    }
    
    async fn get_facets(&self, query: FullTextSearchQuery) -> Result<SearchFacets, FacetError> {
        todo!()
    }
    
    async fn more_like_this(&self, document_id: &str, limit: usize) -> Result<FullTextSearchResults, FullTextSearchError> {
        self.search(FullTextSearchQuery::default()).await
    }
    
    async fn search_with_scroll(&self, query: FullTextSearchQuery) -> Result<ScrollSearchResponse, FullTextSearchError> {
        todo!()
    }
    
    async fn continue_scroll(&self, scroll_id: &str) -> Result<ScrollSearchResponse, FullTextSearchError> {
        todo!()
    }
}

pub struct MockQueryAnalyzer {
    pub should_fail: bool,
}

impl MockQueryAnalyzer {
    pub fn new() -> Self {
        Self { should_fail: false }
    }
    
    pub fn failing(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl QueryAnalyzerPort for MockQueryAnalyzer {
    async fn analyze_query(&self, command: AnalyzeQueryPerformanceCommand) -> Result<QueryPerformanceAnalysis, AnalysisError> {
        if self.should_fail {
            return Err(AnalysisError::QueryParseFailed("Mock analysis failed".to_string()));
        }
        
        Ok(QueryPerformanceAnalysis {
            analysis: QueryAnalysis::default(),
            metrics: QueryPerformanceMetrics::default(),
            execution_plan: None,
            index_stats: None,
        })
    }
    
    async fn parse_query(&self, query: &str, mode: SearchMode) -> Result<ParsedQuery, ParseError> {
        Ok(ParsedQuery::default())
    }
    
    async fn optimize_query(&self, parsed_query: ParsedQuery) -> Result<OptimizedQuery, OptimizationError> {
        Ok(OptimizedQuery::default())
    }
    
    async fn extract_query_terms(&self, query: &str, language: Option<&str>) -> Result<QueryTerms, ExtractionError> {
        Ok(QueryTerms::default())
    }
    
    async fn rewrite_query(&self, parsed_query: ParsedQuery) -> Result<RewrittenQuery, RewriteError> {
        todo!()
    }
}

pub struct MockRelevanceScorer {
    pub should_fail: bool,
}

impl MockRelevanceScorer {
    pub fn new() -> Self {
        Self { should_fail: false }
    }
    
    pub fn failing(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl RelevanceScorerPort for MockRelevanceScorer {
    async fn calculate_score(&self, request: ScoreCalculationRequest) -> Result<RelevanceScore, ScoreError> {
        if self.should_fail {
            return Err(ScoreError::ScoreCalculationFailed("Mock scoring failed".to_string()));
        }
        
        Ok(RelevanceScore::default())
    }
    
    async fn calculate_bm25_score(&self, request: BM25Request) -> Result<f32, ScoreError> {
        Ok(1.0)
    }
    
    async fn calculate_tfidf_score(&self, request: TFIDFRequest) -> Result<f32, ScoreError> {
        Ok(1.0)
    }
    
    async fn combine_scores(&self, scores: Vec<ScoreComponent>) -> Result<CombinedScore, ScoreError> {
        Ok(CombinedScore::default())
    }
    
    async fn normalize_scores(&self, scores: Vec<RawScore>) -> Result<Vec<NormalizedScore>, ScoreError> {
        Ok(vec![])
    }
    
    async fn rank_documents(&self, documents: Vec<DocumentToRank>) -> Result<Vec<RankedDocument>, RankingError> {
        Ok(vec![])
    }
}

pub struct MockHighlighter {
    pub should_fail: bool,
}

impl MockHighlighter {
    pub fn new() -> Self {
        Self { should_fail: false }
    }
    
    pub fn failing(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl HighlighterPort for MockHighlighter {
    async fn generate_highlights(&self, request: HighlightRequest) -> Result<Vec<Highlight>, HighlightError> {
        if self.should_fail {
            return Err(HighlightError::HighlightGenerationFailed("Mock highlighting failed".to_string()));
        }
        
        Ok(vec![])
    }
    
    async fn generate_snippets(&self, request: SnippetRequest) -> Result<Vec<TextSnippet>, HighlightError> {
        Ok(vec![])
    }
    
    async fn extract_best_passages(&self, request: PassageExtractionRequest) -> Result<Vec<TextPassage>, PassageError> {
        todo!()
    }
    
    async fn highlight_terms(&self, text: &str, terms: &[String], language: Option<&str>) -> Result<String, HighlightError> {
        Ok(text.to_string())
    }
}

pub struct MockSearchPerformanceMonitor {
    pub should_fail: bool,
}

impl MockSearchPerformanceMonitor {
    pub fn new() -> Self {
        Self { should_fail: false }
    }
    
    pub fn failing(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl SearchPerformanceMonitorPort for MockSearchPerformanceMonitor {
    async fn record_query_metrics(&self, metrics: QueryMetrics) -> Result<(), MonitoringError> {
        if self.should_fail {
            return Err(MonitoringError::MetricsRecordingFailed("Mock monitoring failed".to_string()));
        }
        
        Ok(())
    }
    
    async fn get_search_stats(&self, time_range: TimeRange) -> Result<SearchPerformanceStats, StatsError> {
        Ok(SearchPerformanceStats::default())
    }
    
    async fn get_slow_queries(&self, threshold_ms: u64, limit: usize) -> Result<Vec<SlowQueryInfo>, QueryError> {
        Ok(vec![])
    }
    
    async fn monitor_search_health(&self) -> Result<SearchHealthStatus, HealthError> {
        Ok(SearchHealthStatus::default())
    }
    
    async fn get_query_patterns(&self, time_range: TimeRange) -> Result<QueryPatterns, PatternError> {
        todo!()
    }
}

pub struct MockSearchIndexManager {
    pub should_fail: bool,
}

impl MockSearchIndexManager {
    pub fn new() -> Self {
        Self { should_fail: false }
    }
    
    pub fn failing(mut self) -> Self {
        self.should_fail = true;
        self
    }
}

#[async_trait]
impl SearchIndexManagerPort for MockSearchIndexManager {
    async fn get_index_stats(&self) -> Result<IndexStats, IndexManagerError> {
        if self.should_fail {
            return Err(IndexManagerError::InternalError("Mock index manager failed".to_string()));
        }
        
        Ok(IndexStats::default())
    }
    
    async fn optimize_index(&self) -> Result<OptimizationResult, OptimizationError> {
        Ok(OptimizationResult::default())
    }
    
    async fn rebuild_index(&self) -> Result<RebuildResult, RebuildError> {
        Ok(RebuildResult::default())
    }
    
    async fn clear_index(&self) -> Result<ClearResult, ClearError> {
        Ok(ClearResult::default())
    }
    
    async fn validate_index(&self) -> Result<ValidationResult, ValidationError> {
        Ok(ValidationResult::default())
    }
    
    async fn get_index_config(&self) -> Result<IndexConfig, ConfigError> {
        Ok(IndexConfig::default())
    }
}

// Test data helpers
fn create_test_search_result() -> SearchResult {
    SearchResult {
        document_id: "test-doc-1".to_string(),
        metadata: ArtifactMetadata {
            title: Some("Test Document".to_string()),
            description: Some("A test document for testing".to_string()),
            tags: vec!["test".to_string(), "sample".to_string()],
            artifact_type: "jar".to_string(),
            version: "1.0.0".to_string(),
            custom_metadata: std::collections::HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        },
        score: 1.0,
        highlights: vec![],
        snippets: vec![],
        ranking: RankingInfo::default(),
        language: Some("en".to_string()),
        indexed_at: chrono::Utc::now(),
    }
}

fn create_test_search_query() -> FullTextSearchQuery {
    FullTextSearchQuery {
        q: "test query".to_string(),
        artifact_type: Some("jar".to_string()),
        language: Some("en".to_string()),
        tags: Some(vec!["test".to_string()]),
        date_range: None,
        search_mode: SearchMode::Simple,
        page: Some(1),
        page_size: Some(10),
        include_highlights: true,
        include_snippets: true,
        snippet_length: Some(150),
        sort_order: SortOrder::Relevance,
        min_score: Some(0.1),
        fuzziness: None,
        enable_stemming: Some(true),
        enable_phonetic: None,
    }
}

// Unit tests
#[tokio::test]
async fn test_full_text_search_use_case_success() {
    let search_adapter = Arc::new(MockFullTextSearchAdapter::new()
        .with_results(vec![create_test_search_result()]));
    let relevance_scorer = Arc::new(MockRelevanceScorer::new());
    let highlighter = Arc::new(MockHighlighter::new());
    let performance_monitor = Arc::new(MockSearchPerformanceMonitor::new());
    let index_manager = Arc::new(MockSearchIndexManager::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_adapter,
        relevance_scorer,
        highlighter,
        performance_monitor,
        index_manager,
    );
    
    let query = create_test_search_query();
    let result = use_case.execute(query).await;
    
    assert!(result.is_ok());
    let search_results = result.unwrap();
    assert_eq!(search_results.results.len(), 1);
    assert_eq!(search_results.total_count, 1);
}

#[tokio::test]
async fn test_full_text_search_use_case_failure() {
    let search_adapter = Arc::new(MockFullTextSearchAdapter::new().failing());
    let relevance_scorer = Arc::new(MockRelevanceScorer::new());
    let highlighter = Arc::new(MockHighlighter::new());
    let performance_monitor = Arc::new(MockSearchPerformanceMonitor::new());
    let index_manager = Arc::new(MockSearchIndexManager::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_adapter,
        relevance_scorer,
        highlighter,
        performance_monitor,
        index_manager,
    );
    
    let query = create_test_search_query();
    let result = use_case.execute(query).await;
    
    assert!(result.is_err());
    match result.unwrap_err() {
        FullTextSearchError::Search { source } => {
            assert_eq!(source.to_string(), "Query execution failed: Mock search failed");
        }
        _ => panic!("Expected Search error"),
    }
}

#[tokio::test]
async fn test_search_suggestions_use_case() {
    let search_adapter = Arc::new(MockFullTextSearchAdapter::new());
    let use_case = SearchSuggestionsUseCase::new(search_adapter);
    
    let query = SearchSuggestionsQuery {
        partial_query: "test".to_string(),
        limit: Some(5),
        ..Default::default()
    };
    
    let result = use_case.execute(query).await;
    
    assert!(result.is_ok());
    let suggestions = result.unwrap();
    assert_eq!(suggestions.total_count, 0);
}

#[tokio::test]
async fn test_query_performance_use_case() {
    let search_adapter = Arc::new(MockFullTextSearchAdapter::new());
    let query_analyzer = Arc::new(MockQueryAnalyzer::new());
    let performance_monitor = Arc::new(MockSearchPerformanceMonitor::new());
    
    let use_case = QueryPerformanceUseCase::new(query_analyzer, search_adapter, performance_monitor);
    
    let command = AnalyzeQueryPerformanceCommand {
        query: create_test_search_query(),
        include_timing: true,
        include_execution_plan: false,
        include_index_stats: false,
    };
    
    let result = use_case.execute(command).await;
    
    assert!(result.is_ok());
    let analysis = result.unwrap();
    assert_eq!(analysis.analysis.complexity_score, 0.0);
}

#[tokio::test]
async fn test_search_api_success() {
    let search_use_case = Arc::new(FullTextSearchUseCase::new(
        Arc::new(MockFullTextSearchAdapter::new().with_results(vec![create_test_search_result()])),
        Arc::new(MockRelevanceScorer::new()),
        Arc::new(MockHighlighter::new()),
        Arc::new(MockSearchPerformanceMonitor::new()),
        Arc::new(MockSearchIndexManager::new()),
    ));
    
    let api = FullTextSearchApi::new(
        search_use_case,
        Arc::new(SearchSuggestionsUseCase::new(Arc::new(MockFullTextSearchAdapter::new()))),
        Arc::new(QueryPerformanceUseCase::new(
            Arc::new(MockQueryAnalyzer::new()),
            Arc::new(MockFullTextSearchAdapter::new()),
            Arc::new(MockSearchPerformanceMonitor::new()),
        )),
    );
    
    let query = create_test_search_query();
    let result = api.search(query).await;
    
    assert!(result.is_ok());
    let search_results = result.unwrap();
    assert_eq!(search_results.results.len(), 1);
}

#[tokio::test]
async fn test_search_api_error_handling() {
    let search_use_case = Arc::new(FullTextSearchUseCase::new(
        Arc::new(MockFullTextSearchAdapter::new().failing()),
        Arc::new(MockRelevanceScorer::new()),
        Arc::new(MockHighlighter::new()),
        Arc::new(MockSearchPerformanceMonitor::new()),
        Arc::new(MockSearchIndexManager::new()),
    ));
    
    let api = FullTextSearchApi::new(
        search_use_case,
        Arc::new(SearchSuggestionsUseCase::new(Arc::new(MockFullTextSearchAdapter::new()))),
        Arc::new(QueryPerformanceUseCase::new(
            Arc::new(MockQueryAnalyzer::new()),
            Arc::new(MockFullTextSearchAdapter::new()),
            Arc::new(MockSearchPerformanceMonitor::new()),
        )),
    );
    
    let query = create_test_search_query();
    let result = api.search(query).await;
    
    assert!(result.is_err());
}

#[tokio::test]
async fn test_api_response_format() {
    let search_use_case = Arc::new(FullTextSearchUseCase::new(
        Arc::new(MockFullTextSearchAdapter::new().with_results(vec![create_test_search_result()])),
        Arc::new(MockRelevanceScorer::new()),
        Arc::new(MockHighlighter::new()),
        Arc::new(MockSearchPerformanceMonitor::new()),
        Arc::new(MockSearchIndexManager::new()),
    ));
    
    let api = FullTextSearchApi::new(
        search_use_case,
        Arc::new(SearchSuggestionsUseCase::new(Arc::new(MockFullTextSearchAdapter::new()))),
        Arc::new(QueryPerformanceUseCase::new(
            Arc::new(MockQueryAnalyzer::new()),
            Arc::new(MockFullTextSearchAdapter::new()),
            Arc::new(MockSearchPerformanceMonitor::new()),
        )),
    );
    
    let request = SearchRequest {
        query: "test".to_string(),
        page_size: Some(10),
        ..Default::default()
    };
    
    let response = api.search_api(request).await;
    
    assert!(response.success);
    assert!(response.data.is_some());
    assert!(response.error.is_none());
    assert!(response.metadata.is_some());
}

#[tokio::test]
async fn test_di_container_builder() {
    let container = SearchFullTextDIContainerBuilder::new()
        .with_search_adapter(Arc::new(MockFullTextSearchAdapter::new()))
        .with_query_analyzer(Arc::new(MockQueryAnalyzer::new()))
        .with_relevance_scorer(Arc::new(MockRelevanceScorer::new()))
        .with_highlighter(Arc::new(MockHighlighter::new()))
        .with_performance_monitor(Arc::new(MockSearchPerformanceMonitor::new()))
        .with_index_manager(Arc::new(MockSearchIndexManager::new()))
        .build()
        .unwrap();
    
    let health = container.search_api().health_check().await;
    assert_eq!(health.overall_status, HealthStatus::Healthy);
}

#[tokio::test]
async fn test_di_container_builder_missing_dependency() {
    let result = SearchFullTextDIContainerBuilder::new()
        .with_search_adapter(Arc::new(MockFullTextSearchAdapter::new()))
        .build();
    
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "query_analyzer is required");
}

#[tokio::test]
async fn test_search_feature_initialization() {
    let di_container = Arc::new(SearchFullTextDIContainer::for_testing());
    let feature = SearchFullTextFeature::new(di_container);
    
    let result = feature.initialize().await;
    assert!(result.is_ok());
    
    let health = feature.health_check().await;
    assert!(health.is_healthy);
}

#[tokio::test]
async fn test_search_feature_statistics() {
    let di_container = Arc::new(SearchFullTextDIContainer::for_testing());
    let feature = SearchFullTextFeature::new(di_container);
    
    let stats = feature.get_statistics().await;
    assert_eq!(stats.feature_name, "search_full_text");
    assert_eq!(stats.total_queries, 0);
}

// Integration tests with tracing
#[traced_test]
#[tokio::test]
async fn test_search_with_tracing() {
    let search_adapter = Arc::new(MockFullTextSearchAdapter::new()
        .with_results(vec![create_test_search_result()]));
    let relevance_scorer = Arc::new(MockRelevanceScorer::new());
    let highlighter = Arc::new(MockHighlighter::new());
    let performance_monitor = Arc::new(MockSearchPerformanceMonitor::new());
    let index_manager = Arc::new(MockSearchIndexManager::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_adapter,
        relevance_scorer,
        highlighter,
        performance_monitor,
        index_manager,
    );
    
    let query = create_test_search_query();
    let result = use_case.execute(query).await;
    
    assert!(result.is_ok());
    
    // Check that tracing logs were captured
    assert!(logs_contain("Executing search query: test query"));
    assert!(logs_contain("Search query completed successfully"));
}

// Performance tests
#[tokio::test]
async fn test_search_performance() {
    let search_adapter = Arc::new(MockFullTextSearchAdapter::new()
        .with_results((0..100).map(|i| {
            let mut result = create_test_search_result();
            result.document_id = format!("test-doc-{}", i);
            result.metadata.title = Some(format!("Test Document {}", i));
            result
        }).collect()));
    
    let relevance_scorer = Arc::new(MockRelevanceScorer::new());
    let highlighter = Arc::new(MockHighlighter::new());
    let performance_monitor = Arc::new(MockSearchPerformanceMonitor::new());
    let index_manager = Arc::new(MockSearchIndexManager::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_adapter,
        relevance_scorer,
        highlighter,
        performance_monitor,
        index_manager,
    );
    
    let query = FullTextSearchQuery {
        q: "test".to_string(),
        page_size: Some(100),
        ..Default::default()
    };
    
    let start = std::time::Instant::now();
    let result = use_case.execute(query).await;
    let duration = start.elapsed();
    
    assert!(result.is_ok());
    let search_results = result.unwrap();
    assert_eq!(search_results.results.len(), 100);
    
    // Performance assertion - should complete in under 100ms
    assert!(duration.as_millis() < 100, "Search took {}ms", duration.as_millis());
}

// Error handling tests
#[tokio::test]
async fn test_error_propagation() {
    let search_adapter = Arc::new(MockFullTextSearchAdapter::new().failing());
    let query_analyzer = Arc::new(MockQueryAnalyzer::new().failing());
    let relevance_scorer = Arc::new(MockRelevanceScorer::new());
    let highlighter = Arc::new(MockHighlighter::new().failing());
    let performance_monitor = Arc::new(MockSearchPerformanceMonitor::new().failing());
    let index_manager = Arc::new(MockSearchIndexManager::new().failing());
    
    let use_case = FullTextSearchUseCase::new(
        search_adapter,
        relevance_scorer,
        highlighter,
        performance_monitor,
        index_manager,
    );
    
    let query = create_test_search_query();
    let result = use_case.execute(query).await;
    
    assert!(result.is_err());
    
    // Test error conversion
    let api = FullTextSearchApi::new(
        Arc::new(FullTextSearchUseCase::new(
            Arc::new(MockFullTextSearchAdapter::new().failing()),
            relevance_scorer,
            highlighter,
            performance_monitor,
            index_manager,
        )),
        Arc::new(SearchSuggestionsUseCase::new(Arc::new(MockFullTextSearchAdapter::new()))),
        Arc::new(QueryPerformanceUseCase::new(
            query_analyzer,
            Arc::new(MockFullTextSearchAdapter::new()),
            Arc::new(MockSearchPerformanceMonitor::new()),
        )),
    );
    
    let response = api.search_api(SearchRequest {
        query: "test".to_string(),
        ..Default::default()
    }).await;
    
    assert!(!response.success);
    assert!(response.error.is_some());
    assert_eq!(response.error.as_ref().unwrap().code, "SEARCH_ERROR");
}

// Boundary tests
#[tokio::test]
async fn test_search_boundary_conditions() {
    let search_adapter = Arc::new(MockFullTextSearchAdapter::new());
    let relevance_scorer = Arc::new(MockRelevanceScorer::new());
    let highlighter = Arc::new(MockHighlighter::new());
    let performance_monitor = Arc::new(MockSearchPerformanceMonitor::new());
    let index_manager = Arc::new(MockSearchIndexManager::new());
    
    let use_case = FullTextSearchUseCase::new(
        search_adapter,
        relevance_scorer,
        highlighter,
        performance_monitor,
        index_manager,
    );
    
    // Test empty query
    let empty_query = FullTextSearchQuery {
        q: "".to_string(),
        ..Default::default()
    };
    let result = use_case.execute(empty_query).await;
    assert!(result.is_ok());
    
    // Test very large page size
    let large_page_query = FullTextSearchQuery {
        q: "test".to_string(),
        page_size: Some(10000),
        ..Default::default()
    };
    let result = use_case.execute(large_page_query).await;
    assert!(result.is_ok());
    
    // Test negative page
    let negative_page_query = FullTextSearchQuery {
        q: "test".to_string(),
        page: Some(0),
        ..Default::default()
    };
    let result = use_case.execute(negative_page_query).await;
    assert!(result.is_ok());
}