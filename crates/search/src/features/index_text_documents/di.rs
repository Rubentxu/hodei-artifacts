//! Dependency Injection Container for Index Text Documents Feature
//!
//! This module provides flexible dependency injection configuration for the
//! index text documents feature, supporting multiple environments and testing.

use std::sync::Arc;
use super::ports::*;
use super::use_case::*;
use super::adapter::*;
// no REST exposure from features
use super::error::*;

/// Dependency injection container for index text documents feature
pub struct IndexTextDocumentsDIContainer {
    pub document_use_case: Arc<IndexDocumentUseCase>,
    pub batch_use_case: Arc<IndexDocumentUseCase>,
    pub text_analyzer: Arc<dyn TextAnalyzerPort>,
    pub health_monitor: Arc<dyn IndexHealthMonitorPort>,
    pub state: IndexTextDocumentsState,
}

impl IndexTextDocumentsDIContainer {
    /// Create a new container with custom dependencies
    pub fn new(
        document_indexer: Arc<dyn DocumentIndexerPort>,
        text_analyzer: Arc<dyn TextAnalyzerPort>,
        health_monitor: Arc<dyn IndexHealthMonitorPort>,
    ) -> Self {
        // No-op implementations for missing ports
        struct NoopIndexSchemaManager;
        #[async_trait::async_trait]
        impl IndexSchemaManagerPort for NoopIndexSchemaManager {
            async fn create_schema(&self, _config: SchemaConfig) -> Result<SchemaInfo, SchemaError> {
                Ok(SchemaInfo { name: "default".into(), version: "1.0.0".into(), fields: vec![], settings: IndexSettings { number_of_shards: 1, number_of_replicas: 0, refresh_interval: "1s".into(), analysis: AnalysisSettings { analyzers: vec![], tokenizers: vec![], filters: vec![] } }, created_at: chrono::Utc::now(), updated_at: chrono::Utc::now() })
            }
            async fn get_schema(&self) -> Result<SchemaInfo, SchemaError> {
                self.create_schema(SchemaConfig { name: "default".into(), fields: vec![], settings: IndexSettings { number_of_shards: 1, number_of_replicas: 0, refresh_interval: "1s".into(), analysis: AnalysisSettings { analyzers: vec![], tokenizers: vec![], filters: vec![] } } }).await
            }
            async fn add_field(&self, field_config: FieldConfig) -> Result<FieldInfo, SchemaError> {
                Ok(FieldInfo { name: field_config.name, field_type: field_config.field_type, indexed: field_config.indexed, stored: field_config.stored, required: field_config.required, options: field_config.options })
            }
            async fn update_field(&self, field_name: &str, field_config: FieldConfig) -> Result<FieldInfo, SchemaError> {
                self.add_field(FieldConfig { name: field_name.into(), ..field_config }).await
            }
            async fn validate_schema(&self) -> Result<SchemaValidationResult, SchemaError> {
                Ok(SchemaValidationResult { is_valid: true, errors: vec![], warnings: vec![] })
            }
        }

        struct NoopDocumentValidator;
        #[async_trait::async_trait]
        impl DocumentValidatorPort for NoopDocumentValidator {
            async fn validate_document(&self, _command: &IndexDocumentCommand) -> Result<ValidationResult, ValidationError> {
                Ok(ValidationResult { is_valid: true, errors: vec![], warnings: vec![] })
            }
            async fn validate_metadata(&self, _metadata: &ArtifactMetadata) -> Result<MetadataValidationResult, ValidationError> {
                Ok(MetadataValidationResult { is_valid: true, errors: vec![], warnings: vec![] })
            }
            async fn validate_content(&self, _content: &str) -> Result<ContentValidationResult, ValidationError> {
                Ok(ContentValidationResult { is_valid: true, content_length: 0, errors: vec![], warnings: vec![] })
            }
            async fn check_duplicate_content(&self, _content: &str) -> Result<bool, DuplicateError> { Ok(false) }
        }

        let schema_manager = Arc::new(NoopIndexSchemaManager) as Arc<dyn IndexSchemaManagerPort>;
        let validator = Arc::new(NoopDocumentValidator) as Arc<dyn DocumentValidatorPort>;

        struct NoopIndexHealthMonitor;
        #[async_trait::async_trait]
        impl IndexHealthMonitorPort for NoopIndexHealthMonitor {
            async fn check_index_health(&self) -> Result<IndexHealth, HealthError> {
                Ok(IndexHealth {
                    status: HealthStatus::Healthy,
                    document_count: 0,
                    index_size_bytes: 0,
                    memory_usage_bytes: 0,
                    last_updated: chrono::Utc::now(),
                    details: vec![],
                })
            }
            async fn get_index_stats(&self) -> Result<IndexStats, StatsError> {
                Ok(IndexStats {
                    total_documents: 0,
                    total_terms: 0,
                    avg_terms_per_document: 0.0,
                    index_size_bytes: 0,
                    memory_usage_bytes: 0,
                    segment_count: 0,
                    created_at: chrono::Utc::now(),
                    last_optimized_at: None,
                })
            }
            async fn get_indexing_performance_metrics(&self, _time_range: TimeRange) -> Result<IndexingMetrics, MetricsError> {
                Ok(IndexingMetrics {
                    avg_indexing_time_ms: 0.0,
                    total_operations: 0,
                    successful_operations: 0,
                    failed_operations: 0,
                    operations_per_second: 0.0,
                    p99_latency_ms: 0.0,
                })
            }
            async fn get_memory_usage(&self) -> Result<MemoryUsage, MemoryError> {
                Ok(MemoryUsage {
                    current_usage_bytes: 0,
                    peak_usage_bytes: 0,
                    memory_limit_bytes: 0,
                    usage_percentage: 0.0,
                })
            }
            async fn needs_optimization(&self) -> Result<bool, OptimizationError> { Ok(false) }
        }

        let document_use_case = Arc::new(IndexDocumentUseCase::new(
            document_indexer.clone(),
            text_analyzer.clone(),
            health_monitor.clone(),
            schema_manager.clone(),
            validator.clone(),
        ));
        let batch_use_case = Arc::new(IndexDocumentUseCase::new(
            document_indexer.clone(),
            text_analyzer.clone(),
            health_monitor.clone(),
            schema_manager.clone(),
            validator.clone(),
        ));
        
        let state = IndexTextDocumentsState {
            document_use_case: document_use_case.clone(),
            batch_use_case: batch_use_case.clone(),
            text_analyzer: text_analyzer.clone(),
            health_monitor: health_monitor.clone(),
        };
        
        Self {
            document_use_case,
            batch_use_case,
            text_analyzer,
            health_monitor,
            state,
        }
    }
    
    /// Create container for production environment with file-based index
    pub fn for_production_with_file_index(
        index_path: &std::path::Path,
    ) -> Result<Self, IndexDocumentError> {
        let document_indexer = Arc::new(TantivyDocumentIndexer::new(Some(index_path))?);
        let text_analyzer = Arc::new(BasicTextAnalyzer::new());
        let health_monitor = Arc::new(BasicIndexHealthMonitor::new(document_indexer.index_arc()));
        
        Ok(Self::new(document_indexer, text_analyzer, health_monitor))
    }
    
    /// Create container for production environment with in-memory index
    pub fn for_production_with_memory_index() -> Result<Self, IndexDocumentError> {
        let document_indexer = Arc::new(TantivyDocumentIndexer::new(None)?);
        let text_analyzer = Arc::new(BasicTextAnalyzer::new());
        let health_monitor = Arc::new(BasicIndexHealthMonitor::new(document_indexer.index_arc()));
        
        Ok(Self::new(document_indexer, text_analyzer, health_monitor))
    }
    
    /// Create container for testing environment
    #[cfg(test)]
    pub fn for_testing() -> Self {
        use super::adapter::test::*;
        
        let document_indexer = Arc::new(MockDocumentIndexer::new());
        let text_analyzer = Arc::new(MockTextAnalyzer::new());
        let health_monitor = Arc::new(MockIndexHealthMonitor::new());
        
        Self::new(document_indexer, text_analyzer, health_monitor)
    }
    
    /// Create container for integration testing with real dependencies
    pub fn for_integration_testing(
        index_path: Option<&std::path::Path>,
    ) -> Result<Self, IndexDocumentError> {
        let document_indexer = Arc::new(TantivyDocumentIndexer::new(index_path)?);
        let text_analyzer = Arc::new(BasicTextAnalyzer::new());
        let health_monitor = Arc::new(BasicIndexHealthMonitor::new(document_indexer.index_arc()));
        
        Ok(Self::new(document_indexer, text_analyzer, health_monitor))
    }
    
    // No API router exposure from DI per architecture guidelines
    
    /// Get a reference to the document use case
    pub fn document_use_case(&self) -> &Arc<IndexDocumentUseCase> {
        &self.document_use_case
    }
    
    /// Get a reference to the batch use case
    pub fn batch_use_case(&self) -> &Arc<IndexDocumentUseCase> {
        &self.batch_use_case
    }
    
    /// Get a reference to the text analyzer
    pub fn text_analyzer(&self) -> &Arc<dyn TextAnalyzerPort> {
        &self.text_analyzer
    }
    
    /// Get a reference to the health monitor
    pub fn health_monitor(&self) -> &Arc<dyn IndexHealthMonitorPort> {
        &self.health_monitor
    }
    
    /// Get the application state
    pub fn state(&self) -> &IndexTextDocumentsState {
        &self.state
    }
}

/// Builder pattern for creating DI containers with custom configuration
pub struct IndexTextDocumentsDIContainerBuilder {
    document_indexer: Option<Arc<dyn DocumentIndexerPort>>,
    text_analyzer: Option<Arc<dyn TextAnalyzerPort>>,
    health_monitor: Option<Arc<dyn IndexHealthMonitorPort>>,
}

impl IndexTextDocumentsDIContainerBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            document_indexer: None,
            text_analyzer: None,
            health_monitor: None,
        }
    }
    
    /// Set the document indexer
    pub fn with_document_indexer(mut self, indexer: Arc<dyn DocumentIndexerPort>) -> Self {
        self.document_indexer = Some(indexer);
        self
    }
    
    /// Set the text analyzer
    pub fn with_text_analyzer(mut self, analyzer: Arc<dyn TextAnalyzerPort>) -> Self {
        self.text_analyzer = Some(analyzer);
        self
    }
    
    /// Set the health monitor
    pub fn with_health_monitor(mut self, monitor: Arc<dyn IndexHealthMonitorPort>) -> Self {
        self.health_monitor = Some(monitor);
        self
    }
    
    /// Build the container
    pub fn build(self) -> Result<IndexTextDocumentsDIContainer, IndexDocumentError> {
        let document_indexer = self.document_indexer.ok_or_else(|| {
            IndexDocumentError::configuration("Document indexer not provided")
        })?;
        
        let text_analyzer = self.text_analyzer.ok_or_else(|| {
            IndexDocumentError::configuration("Text analyzer not provided")
        })?;
        
        let health_monitor = self.health_monitor.ok_or_else(|| {
            IndexDocumentError::configuration("Health monitor not provided")
        })?;
        
        Ok(IndexTextDocumentsDIContainer::new(
            document_indexer,
            text_analyzer,
            health_monitor,
        ))
    }
    
    /// Build with production defaults (file-based index)
    pub fn build_with_production_defaults(self, index_path: &std::path::Path) -> Result<IndexTextDocumentsDIContainer, IndexDocumentError> {
        let document_indexer = self.document_indexer
            .unwrap_or_else(|| Arc::new(TantivyDocumentIndexer::new(Some(index_path)).unwrap()));
        
        let text_analyzer = self.text_analyzer
            .unwrap_or_else(|| Arc::new(BasicTextAnalyzer::new()));
        
        let health_monitor = self.health_monitor
            .unwrap_or_else(|| Arc::new(NoopIndexHealthMonitor));
        
        Ok(IndexTextDocumentsDIContainer::new(
            document_indexer,
            text_analyzer,
            health_monitor,
        ))
    }
    
    /// Build with production defaults (in-memory index)
    pub fn build_with_memory_defaults(self) -> Result<IndexTextDocumentsDIContainer, IndexDocumentError> {
        let document_indexer = self.document_indexer
            .unwrap_or_else(|| Arc::new(TantivyDocumentIndexer::new(None).unwrap()));
        
        let text_analyzer = self.text_analyzer
            .unwrap_or_else(|| Arc::new(BasicTextAnalyzer::new()));
        
        let health_monitor = self.health_monitor
            .unwrap_or_else(|| Arc::new(NoopIndexHealthMonitor));
        
        Ok(IndexTextDocumentsDIContainer::new(
            document_indexer,
            text_analyzer,
            health_monitor,
        ))
    }
    
    /// Build with testing defaults
    #[cfg(test)]
    pub fn build_with_testing_defaults(self) -> IndexTextDocumentsDIContainer {
        use super::adapter::test::*;
        
        let document_indexer = self.document_indexer
            .unwrap_or_else(|| Arc::new(MockDocumentIndexer::new()));
        
        let text_analyzer = self.text_analyzer
            .unwrap_or_else(|| Arc::new(MockTextAnalyzer::new()));
        
        let health_monitor = self.health_monitor
            .unwrap_or_else(|| Arc::new(MockIndexHealthMonitor::new()));
        
        IndexTextDocumentsDIContainer::new(
            document_indexer,
            text_analyzer,
            health_monitor,
        )
    }
}

impl Default for IndexTextDocumentsDIContainerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration for the index text documents feature
#[derive(Debug, Clone)]
pub struct IndexTextDocumentsConfig {
    /// Path to the index directory (None for in-memory index)
    pub index_path: Option<std::path::PathBuf>,
    /// Maximum number of concurrent indexing operations
    pub max_concurrent_operations: Option<usize>,
    /// Index buffer size in bytes
    pub index_buffer_size: usize,
    /// Whether to enable text analysis features
    pub enable_text_analysis: bool,
    /// Whether to enable health monitoring
    pub enable_health_monitoring: bool,
    /// Timeout for indexing operations in milliseconds
    pub indexing_timeout_ms: u64,
}

impl Default for IndexTextDocumentsConfig {
    fn default() -> Self {
        Self {
            index_path: None,
            max_concurrent_operations: Some(10),
            index_buffer_size: 50_000_000, // 50MB
            enable_text_analysis: true,
            enable_health_monitoring: true,
            indexing_timeout_ms: 30000, // 30 seconds
        }
    }
}

impl IndexTextDocumentsConfig {
    /// Create configuration for production with file-based index
    pub fn production_with_file_index(index_path: std::path::PathBuf) -> Self {
        Self {
            index_path: Some(index_path),
            ..Default::default()
        }
    }
    
    /// Create configuration for production with in-memory index
    pub fn production_with_memory_index() -> Self {
        Self {
            index_path: None,
            ..Default::default()
        }
    }
    
    /// Create configuration for testing
    pub fn testing() -> Self {
        Self {
            index_path: None,
            max_concurrent_operations: Some(2),
            index_buffer_size: 1_000_000, // 1MB
            enable_text_analysis: true,
            enable_health_monitoring: true,
            indexing_timeout_ms: 1000, // 1 second
        }
    }
    
    /// Create DI container from this configuration
    pub fn create_container(self) -> Result<IndexTextDocumentsDIContainer, IndexDocumentError> {
        let document_indexer = Arc::new(TantivyDocumentIndexer::new(
            self.index_path.as_deref()
        )?);
        
        let text_analyzer = if self.enable_text_analysis {
            Arc::new(BasicTextAnalyzer::new()) as Arc<dyn TextAnalyzerPort>
        } else {
            // Use a no-op analyzer when text analysis is disabled
            todo!("Implement no-op text analyzer")
        };
        
        let health_monitor = if self.enable_health_monitoring {
            Arc::new(BasicIndexHealthMonitor::new(document_indexer.index_arc())) as Arc<dyn IndexHealthMonitorPort>
        } else {
            // Use a no-op health monitor when health monitoring is disabled
            todo!("Implement no-op health monitor")
        };
        
        Ok(IndexTextDocumentsDIContainer::new(
            document_indexer,
            text_analyzer,
            health_monitor,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::index_text_documents::adapter::test::*;
    
    #[test]
    fn test_di_container_builder() {
        let container = IndexTextDocumentsDIContainerBuilder::new()
            .build_with_testing_defaults();
        
        assert!(container.document_use_case().is_ready());
        assert!(container.batch_use_case().is_ready());
        assert!(container.text_analyzer().is_ready());
        assert!(container.health_monitor().is_ready());
    }
    
    #[test]
    fn test_di_container_with_custom_deps() {
        let document_indexer = Arc::new(MockDocumentIndexer::new());
        let text_analyzer = Arc::new(MockTextAnalyzer::new());
        let health_monitor = Arc::new(MockIndexHealthMonitor::new());
        
        let container = IndexTextDocumentsDIContainer::new(
            document_indexer,
            text_analyzer,
            health_monitor,
        );
        
        assert!(container.document_use_case().is_ready());
        assert!(container.batch_use_case().is_ready());
        assert!(container.text_analyzer().is_ready());
        assert!(container.health_monitor().is_ready());
    }
    
    #[test]
    fn test_config_default() {
        let config = IndexTextDocumentsConfig::default();
        assert_eq!(config.index_buffer_size, 50_000_000);
        assert_eq!(config.indexing_timeout_ms, 30000);
        assert!(config.enable_text_analysis);
        assert!(config.enable_health_monitoring);
    }
    
    #[test]
    fn test_config_testing() {
        let config = IndexTextDocumentsConfig::testing();
        assert_eq!(config.index_buffer_size, 1_000_000);
        assert_eq!(config.indexing_timeout_ms, 1000);
        assert!(config.enable_text_analysis);
        assert!(config.enable_health_monitoring);
    }
}