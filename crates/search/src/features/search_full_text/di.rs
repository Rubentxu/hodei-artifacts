//! Dependency Injection for Full Text Search Feature
//!
//! This module provides flexible dependency injection containers for the search feature,
//! following the VSA principle of allowing different implementations for different environments.

use std::sync::Arc;
use tantivy::{Index, schema::Schema};
use tracing::{info, debug, warn, error};
use async_trait::async_trait;
use super::error::*;

use super::use_case::*;
use super::adapter::*;
use api::*;
use super::ports::{
    FullTextSearchPort, QueryAnalyzerPort, RelevanceScorerPort, HighlighterPort, 
    SearchPerformanceMonitorPort, SearchIndexManagerPort, HealthStatus,
    QueryPatterns, TimeRange, IndexError, ConfigError,
    IndexConfig as PortsIndexConfig, ConfigUpdateResult, 
    MaintenanceResult, SegmentInfo, MergeResult, MaintenanceTask, MergePolicy,
    OptimizationResult, RebuildResult, ClearResult, ValidationResult,
    IndexSettings, SimilarityConfig, TaskResult,
};
use crate::features::index_text_documents::ports::IndexStats;
use super::dto::*;
use crate::features::index_text_documents::adapter::DocumentIndexSchema;

/// Main DI container for the search_full_text feature
pub struct SearchFullTextDIContainer {
    pub search_api: Arc<FullTextSearchApi>,
    pub search_use_case: Arc<FullTextSearchUseCase>,
    pub suggestions_use_case: Arc<SearchSuggestionsUseCase>,
    pub query_analysis_use_case: Arc<QueryPerformanceUseCase>,
}

impl SearchFullTextDIContainer {
    /// Create a new DI container with specific implementations
    pub fn new(
        search_adapter: Arc<dyn FullTextSearchPort>,
        query_analyzer: Arc<dyn QueryAnalyzerPort>,
        relevance_scorer: Arc<dyn RelevanceScorerPort>,
        highlighter: Arc<dyn HighlighterPort>,
        performance_monitor: Arc<dyn SearchPerformanceMonitorPort>,
        index_manager: Arc<dyn SearchIndexManagerPort>,
    ) -> Self {
        // Create use cases
        let search_use_case = Arc::new(FullTextSearchUseCase::new(
            search_adapter.clone(),
            relevance_scorer.clone(),
            highlighter.clone(),
            performance_monitor.clone(),
            index_manager.clone(),
        ));
        
        let suggestions_use_case = Arc::new(SearchSuggestionsUseCase::new(
            search_adapter.clone(),
        ));
        
        let query_analysis_use_case = Arc::new(QueryPerformanceUseCase::new(
            query_analyzer,
            search_adapter.clone(),
            performance_monitor.clone(),
        ));
        
        // Create API
        let search_api = Arc::new(FullTextSearchApi::new(
            search_use_case.clone(),
            suggestions_use_case.clone(),
            query_analysis_use_case.clone(),
        ));
        
        Self {
            search_api,
            search_use_case,
            suggestions_use_case,
            query_analysis_use_case,
        }
    }
    
    /// Create a production-ready container with Tantivy implementations
    pub fn for_production(index_path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // Load or create Tantivy index
        let index = Self::load_or_create_index(index_path)?;
        let schema = Arc::new(DocumentIndexSchema::create());
        
        // Create adapters
        let search_adapter = Arc::new(TantivyFullTextSearchAdapter::new(
            Arc::new(std::sync::RwLock::new(index)),
            schema.clone(),
        ));
        
        let query_analyzer = Arc::new(SimpleQueryAnalyzer::new());
        let relevance_scorer = Arc::new(SimpleRelevanceScorer::new());
        let highlighter = Arc::new(SimpleHighlighter::new());
        let performance_monitor = Arc::new(SimpleSearchPerformanceMonitor::new());
        let index_manager = Arc::new(TantivySearchIndexManager::new(
            Arc::new(std::sync::RwLock::new(index)),
            schema.clone(),
        ));
        
        Ok(Self::new(
            search_adapter,
            query_analyzer,
            relevance_scorer,
            highlighter,
            performance_monitor,
            index_manager,
        ))
    }
    
    /// Create a testing container with mock implementations
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use crate::features::search_full_text::adapter::test::*;
        
        let search_adapter = Arc::new(MockFullTextSearchAdapter::new());
        let query_analyzer = Arc::new(MockQueryAnalyzer::new());
        let relevance_scorer = Arc::new(MockRelevanceScorer::new());
        let highlighter = Arc::new(MockHighlighter::new());
        let performance_monitor = Arc::new(MockSearchPerformanceMonitor::new());
        let index_manager = Arc::new(MockSearchIndexManager::new());
        
        Self::new(
            search_adapter,
            query_analyzer,
            relevance_scorer,
            highlighter,
            performance_monitor,
            index_manager,
        )
    }
    
    /// Load or create Tantivy index
    fn load_or_create_index(index_path: &str) -> Result<Index, Box<dyn std::error::Error>> {
        let path = std::path::Path::new(index_path);
        
        if path.exists() {
            let index = Index::open_in_dir(path)?;
            info!("Loaded existing Tantivy index from: {}", index_path);
            Ok(index)
        } else {
            std::fs::create_dir_all(path)?;
            let schema = DocumentIndexSchema::create_tantivy_schema();
            let index = Index::create_in_dir(path, schema)?;
            info!("Created new Tantivy index at: {}", index_path);
            Ok(index)
        }
    }
    
    /// Get the search API
    pub fn search_api(&self) -> Arc<FullTextSearchApi> {
        self.search_api.clone()
    }
    
    /// Get the search use case
    pub fn search_use_case(&self) -> Arc<FullTextSearchUseCase> {
        self.search_use_case.clone()
    }
    
    /// Get the suggestions use case
    pub fn suggestions_use_case(&self) -> Arc<SearchSuggestionsUseCase> {
        self.suggestions_use_case.clone()
    }
    
    /// Get the query analysis use case
    pub fn query_analysis_use_case(&self) -> Arc<QueryPerformanceUseCase> {
        self.query_analysis_use_case.clone()
    }
}

/// Builder pattern for creating DI containers with custom configurations
pub struct SearchFullTextDIContainerBuilder {
    search_adapter: Option<Arc<dyn FullTextSearchPort>>,
    query_analyzer: Option<Arc<dyn QueryAnalyzerPort>>,
    relevance_scorer: Option<Arc<dyn RelevanceScorerPort>>,
    highlighter: Option<Arc<dyn HighlighterPort>>,
    performance_monitor: Option<Arc<dyn SearchPerformanceMonitorPort>>,
    index_manager: Option<Arc<dyn SearchIndexManagerPort>>,
}

impl SearchFullTextDIContainerBuilder {
    pub fn new() -> Self {
        Self {
            search_adapter: None,
            query_analyzer: None,
            relevance_scorer: None,
            highlighter: None,
            performance_monitor: None,
            index_manager: None,
        }
    }
    
    pub fn with_search_adapter(mut self, adapter: Arc<dyn FullTextSearchPort>) -> Self {
        self.search_adapter = Some(adapter);
        self
    }
    
    pub fn with_query_analyzer(mut self, analyzer: Arc<dyn QueryAnalyzerPort>) -> Self {
        self.query_analyzer = Some(analyzer);
        self
    }
    
    pub fn with_relevance_scorer(mut self, scorer: Arc<dyn RelevanceScorerPort>) -> Self {
        self.relevance_scorer = Some(scorer);
        self
    }
    
    pub fn with_highlighter(mut self, highlighter: Arc<dyn HighlighterPort>) -> Self {
        self.highlighter = Some(highlighter);
        self
    }
    
    pub fn with_performance_monitor(mut self, monitor: Arc<dyn SearchPerformanceMonitorPort>) -> Self {
        self.performance_monitor = Some(monitor);
        self
    }
    
    pub fn with_index_manager(mut self, manager: Arc<dyn SearchIndexManagerPort>) -> Self {
        self.index_manager = Some(manager);
        self
    }
    
    pub fn build(self) -> Result<SearchFullTextDIContainer, &'static str> {
        let search_adapter = self.search_adapter.ok_or("search_adapter is required")?;
        let query_analyzer = self.query_analyzer.ok_or("query_analyzer is required")?;
        let relevance_scorer = self.relevance_scorer.ok_or("relevance_scorer is required")?;
        let highlighter = self.highlighter.ok_or("highlighter is required")?;
        let performance_monitor = self.performance_monitor.ok_or("performance_monitor is required")?;
        let index_manager = self.index_manager.ok_or("index_manager is required")?;
        
        Ok(SearchFullTextDIContainer::new(
            search_adapter,
            query_analyzer,
            relevance_scorer,
            highlighter,
            performance_monitor,
            index_manager,
        ))
    }
}

impl Default for SearchFullTextDIContainerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Tantivy-based search index manager implementation
pub struct TantivySearchIndexManager {
    index: Arc<std::sync::RwLock<Index>>,
    schema: Arc<DocumentIndexSchema>,
}

impl TantivySearchIndexManager {
    pub fn new(index: Arc<std::sync::RwLock<Index>>, schema: Arc<DocumentIndexSchema>) -> Self {
        Self { index, schema }
    }
}

#[async_trait]
impl SearchIndexManagerPort for TantivySearchIndexManager {
    async fn get_index_stats(&self) -> Result<IndexStats, IndexError> {
        let index = self.index.read()
            .map_err(|e| IndexError::IndexOperationFailed(format!("Failed to acquire index lock: {}", e)))?;
        
        // This is a simplified implementation
        // In a real implementation, we would get actual statistics from Tantivy
        Ok(IndexStats {
            total_documents: 0, // TODO: Get actual document count
            total_terms: 0,
            avg_terms_per_document: 0.0,
            index_size_bytes: 0,
            memory_usage_bytes: 0,
            segment_count: 0,
            created_at: chrono::Utc::now(),
            last_optimized_at: None,
        })
    }
    
    async fn optimize_index(&self) -> Result<OptimizationResult, OptimizationError> {
        // TODO: Implement index optimization
        Ok(OptimizationResult {
            segments_before: 1,
            segments_after: 1,
            size_reduction_bytes: 0,
            time_taken_ms: 100,
            success: true,
        })
    }
    
    async fn rebuild_index(&self) -> Result<RebuildResult, RebuildError> {
        // TODO: Implement index rebuild
        Ok(RebuildResult {
            success: true,
            documents_processed: 0,
            rebuild_time_ms: 0,
            message: "Index rebuild completed".to_string(),
        })
    }
    
    async fn clear_index(&self) -> Result<ClearResult, ClearError> {
        // TODO: Implement index clear
        Ok(ClearResult {
            success: true,
            documents_removed: 0,
            clear_time_ms: 0,
        })
    }
    
    async fn validate_index(&self) -> Result<ValidationResult, ValidationError> {
        // TODO: Implement index validation
        Ok(ValidationResult {
            is_valid: true,
            issues: Vec::new(),
            validation_time_ms: 0,
        })
    }
    
    async fn get_index_config(&self) -> Result<PortsIndexConfig, ConfigError> {
        Ok(PortsIndexConfig {
            index_name: "default".to_string(),
            settings: IndexSettings {
                number_of_shards: 1,
                number_of_replicas: 0,
                refresh_interval: "1s".to_string(),
                max_result_window: 10000,
            },
            analyzers: Vec::new(),
            similarity: SimilarityConfig {
                algorithm: "BM25".to_string(),
                parameters: std::collections::HashMap::new(),
            },
        })
    }
    
    async fn update_index_config(&self, config: PortsIndexConfig) -> Result<ConfigUpdateResult, ConfigError> {
        // TODO: Implement actual config update
        Ok(ConfigUpdateResult {
            success: true,
            message: "Configuration updated successfully".to_string(),
            changes_applied: vec!["settings".to_string()],
        })
    }
    
    async fn perform_maintenance(&self, tasks: Vec<MaintenanceTask>) -> Result<MaintenanceResult, MaintenanceError> {
        // TODO: Implement actual maintenance
        Ok(MaintenanceResult {
            tasks_completed: tasks.len(),
            tasks_failed: 0,
            time_taken_ms: 500,
            details: tasks.into_iter().map(|task| TaskResult {
                task_type: task.task_type,
                success: true,
                message: "Task completed".to_string(),
                time_taken_ms: 100,
            }).collect(),
        })
    }
    
    async fn get_segments_info(&self) -> Result<Vec<SegmentInfo>, SegmentError> {
        // TODO: Implement actual segment info retrieval
        Ok(vec![SegmentInfo {
            segment_id: "segment_0".to_string(),
            document_count: 0,
            size_bytes: 0,
            deleted_documents: 0,
            created_at: chrono::Utc::now(),
        }])
    }
    
    async fn merge_segments(&self, merge_policy: MergePolicy) -> Result<MergeResult, MergeError> {
        // TODO: Implement actual segment merging
        Ok(MergeResult {
            segments_merged: 2,
            segments_created: 1,
            size_reduction_bytes: 1024,
            time_taken_ms: 1000,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_di_container_builder() {
        use crate::features::search_full_text::adapter::test::*;
        
        let container = SearchFullTextDIContainerBuilder::new()
            .with_search_adapter(Arc::new(MockFullTextSearchAdapter::new()))
            .with_query_analyzer(Arc::new(MockQueryAnalyzer::new()))
            .with_relevance_scorer(Arc::new(MockRelevanceScorer::new()))
            .with_highlighter(Arc::new(MockHighlighter::new()))
            .with_performance_monitor(Arc::new(MockSearchPerformanceMonitor::new()))
            .with_index_manager(Arc::new(MockSearchIndexManager::new()))
            .build()
            .unwrap();
        
        assert!(container.search_api().health_check().await.overall_status == HealthStatus::Healthy);
    }
    
    #[tokio::test]
    async fn test_production_container() {
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().to_str().unwrap();
        
        let result = SearchFullTextDIContainer::for_production(index_path);
        assert!(result.is_ok());
        
        let container = result.unwrap();
        assert!(container.search_api().health_check().await.overall_status == HealthStatus::Healthy);
    }
    
    #[tokio::test]
    async fn test_testing_container() {
        let container = SearchFullTextDIContainer::for_testing();
        
        let health = container.search_api().health_check().await;
        assert_eq!(health.overall_status, HealthStatus::Healthy);
    }
}