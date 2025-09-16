//! Use Case for Index Text Documents Feature
//!
//! This module implements the core business logic for document indexing,
//! following Clean Architecture principles with clear separation of concerns.

use std::sync::Arc;
use tracing::{info, debug, warn, error, instrument, span, Level};
use uuid::Uuid;
use chrono::Utc;

use crate::features::index_text_documents::{
    dto::{
        IndexDocumentCommand, DocumentIndexedResponse, BatchIndexCommand, BatchIndexResponse,
        RemoveDocumentCommand, DocumentRemovedResponse, GetIndexedDocumentsQuery,
        IndexedDocumentsResponse, AnalyzeTextCommand, TextAnalysisResponse,
        IndexingStatus, BatchOperationStatus, RemovalStatus,
    },
    ports::{
        DocumentIndexerPort, TextAnalyzerPort, IndexHealthMonitorPort,
        IndexSchemaManagerPort, DocumentValidatorPort,
    },
    error::{IndexDocumentError, IndexDocumentResult, WithContext, ErrorContext, ToIndexDocumentError},
};

/// Use case for managing document indexing operations
/// 
/// This use case orchestrates the indexing process by coordinating
/// between various ports and implementing business rules.
pub struct IndexDocumentUseCase {
    /// Document indexer implementation
    indexer: Arc<dyn DocumentIndexerPort>,
    /// Text analyzer for linguistic processing
    analyzer: Arc<dyn TextAnalyzerPort>,
    /// Health monitor for index status
    health_monitor: Arc<dyn IndexHealthMonitorPort>,
    /// Schema manager for index configuration
    schema_manager: Arc<dyn IndexSchemaManagerPort>,
    /// Document validator for input validation
    validator: Arc<dyn DocumentValidatorPort>,
}

impl IndexDocumentUseCase {
    /// Create a new instance of the use case
    pub fn new(
        indexer: Arc<dyn DocumentIndexerPort>,
        analyzer: Arc<dyn TextAnalyzerPort>,
        health_monitor: Arc<dyn IndexHealthMonitorPort>,
        schema_manager: Arc<dyn IndexSchemaManagerPort>,
        validator: Arc<dyn DocumentValidatorPort>,
    ) -> Self {
        Self {
            indexer,
            analyzer,
            health_monitor,
            schema_manager,
            validator,
        }
    }

    /// Execute document indexing with full processing pipeline
    #[instrument(skip(self, command), fields(artifact_id = %command.artifact_id, language = ?command.language))]
    pub async fn execute(&self, command: IndexDocumentCommand) -> IndexDocumentResult<DocumentIndexedResponse> {
        info!("Starting document indexing process");
        
        let span = span!(Level::INFO, "index_document", artifact_id = %command.artifact_id);
        let _enter = span.enter();

        // Step 1: Validate document before processing
        debug!("Validating document");
        let validation_result = self.validator.validate_document(&command)
            .await
            .to_index_document_error()?;
        
        if !validation_result.is_valid {
            let error_msg = format!("Document validation failed: {:?}", validation_result.errors);
            error!("{}", error_msg);
            return Err(IndexDocumentError::business_rule_validation(error_msg));
        }

        // Step 2: Check for duplicate content if not forcing re-index
        if !command.force_reindex {
            debug!("Checking for duplicate content");
            let is_duplicate = self.validator.check_duplicate_content(&command.content)
                .await
                .to_index_document_error()?;
            
            if is_duplicate {
                warn!("Duplicate content detected for artifact: {}", command.artifact_id);
                return Err(IndexDocumentError::business_rule_validation(
                    format!("Duplicate content detected for artifact: {}", command.artifact_id)
                ));
            }
        }

        // Step 3: Check index health before processing
        debug!("Checking index health");
        let health = self.health_monitor.check_index_health()
            .await
            .to_index_document_error()?;
        
        if health.status != super::ports::HealthStatus::Healthy {
            warn!("Index health check returned status: {:?}", health.status);
            // Allow proceeding with warnings but fail on unhealthy
            if health.status == super::ports::HealthStatus::Unhealthy {
                return Err(IndexDocumentError::resource_unavailable(
                    "Search index is unhealthy".to_string()
                ));
            }
        }

        // Step 4: Analyze text content
        debug!("Analyzing text content");
        let analyze_command = AnalyzeTextCommand {
            text: command.content.clone(),
            language: command.language.clone(),
            options: Default::default(),
        };
        
        let analysis_result = self.analyzer.analyze_text(analyze_command)
            .await
            .to_index_document_error()?;

        // Step 5: Index the document
        debug!("Indexing document");
        let start_time = std::time::Instant::now();
        let mut indexing_result = self.indexer.index_document(command)
            .await
            .to_index_document_error()?;
        
        let indexing_time_ms = start_time.elapsed().as_millis() as u64;
        indexing_result.indexing_time_ms = indexing_time_ms;
        indexing_result.token_count = analysis_result.token_count;

        // Step 6: Log metrics and return result
        info!(
            artifact_id = %indexing_result.document_id,
            indexing_time_ms = indexing_result.indexing_time_ms,
            token_count = indexing_result.token_count,
            status = ?indexing_result.status,
            "Document indexing completed successfully"
        );

        // Performance warning if indexing took too long
        if indexing_time_ms > 1000 {
            warn!(
                artifact_id = %indexing_result.document_id,
                indexing_time_ms = indexing_time_ms,
                "Document indexing took longer than expected"
            );
        }

        Ok(indexing_result)
    }

    /// Execute batch indexing of multiple documents
    #[instrument(skip(self, command), fields(document_count = command.documents.len()))]
    pub async fn execute_batch(&self, command: BatchIndexCommand) -> IndexDocumentResult<BatchIndexResponse> {
        info!("Starting batch document indexing");
        
        let span = span!(Level::INFO, "batch_index_documents", document_count = command.documents.len());
        let _enter = span.enter();

        let start_time = std::time::Instant::now();
        let mut results = Vec::new();
        let mut success_count = 0;
        let mut failure_count = 0;

        // Process documents in parallel if requested
        if command.parallel_processing {
            let max_concurrency = command.max_concurrency.unwrap_or(4);
            let semaphore = Arc::new(tokio::sync::Semaphore::new(max_concurrency));
            
            let tasks: Vec<_> = command.documents.into_iter().map(|doc| {
                let semaphore = semaphore.clone();
                let indexer = self.indexer.clone();
                let analyzer = self.analyzer.clone();
                let validator = self.validator.clone();
                
                tokio::spawn(async move {
                    let _permit = semaphore.acquire().await.unwrap();
                    Self::process_single_document(doc, indexer, analyzer, validator).await
                })
            }).collect();

            for task in tasks {
                match task.await {
                    Ok(result) => match result {
                        Ok(response) => {
                            success_count += 1;
                            results.push(response);
                        },
                        Err(e) => {
                            failure_count += 1;
                            error!("Failed to process document in batch: {}", e);
                        }
                    },
                    Err(e) => {
                        failure_count += 1;
                        error!("Task execution failed: {}", e);
                    }
                }
            }
        } else {
            // Sequential processing
            for doc in command.documents {
                match Self::process_single_document(doc, self.indexer.clone(), self.analyzer.clone(), self.validator.clone()).await {
                    Ok(response) => {
                        success_count += 1;
                        results.push(response);
                    },
                    Err(e) => {
                        failure_count += 1;
                        error!("Failed to process document in batch: {}", e);
                    }
                }
            }
        }

        let total_time_ms = start_time.elapsed().as_millis() as u64;
        let batch_status = if failure_count == 0 {
            BatchOperationStatus::Completed
        } else if success_count > 0 {
            BatchOperationStatus::PartialSuccess
        } else {
            BatchOperationStatus::Failed
        };

        let response = BatchIndexResponse {
            results,
            batch_status,
            total_time_ms,
            success_count,
            failure_count,
        };

        info!(
            total_time_ms = response.total_time_ms,
            success_count = response.success_count,
            failure_count = response.failure_count,
            batch_status = ?response.batch_status,
            "Batch indexing completed"
        );

        Ok(response)
    }

    /// Execute document removal from index
    #[instrument(skip(self, command), fields(document_id = %command.document_id))]
    pub async fn execute_remove(&self, command: RemoveDocumentCommand) -> IndexDocumentResult<DocumentRemovedResponse> {
        info!("Starting document removal");
        
        let span = span!(Level::INFO, "remove_document", document_id = %command.document_id);
        let _enter = span.enter();

        let start_time = std::time::Instant::now();
        
        // Check if document exists
        let exists = self.indexer.document_exists(&command.document_id)
            .await
            .to_index_document_error()?;
        
        if !exists {
            warn!("Document not found for removal: {}", command.document_id);
            return Ok(DocumentRemovedResponse {
                document_id: command.document_id,
                status: RemovalStatus::NotFound,
                removal_time_ms: start_time.elapsed().as_millis() as u64,
            });
        }

        // Remove the document
        debug!("Removing document from index");
        let mut result = self.indexer.remove_document(command)
            .await
            .to_index_document_error()?;
        
        result.removal_time_ms = start_time.elapsed().as_millis() as u64;

        info!(
            document_id = %result.document_id,
            removal_time_ms = result.removal_time_ms,
            status = ?result.status,
            "Document removal completed"
        );

        Ok(result)
    }

    /// Execute query for indexed documents
    #[instrument(skip(self, query))]
    pub async fn execute_get_documents(&self, query: GetIndexedDocumentsQuery) -> IndexDocumentResult<IndexedDocumentsResponse> {
        info!("Querying indexed documents");
        
        let span = span!(Level::INFO, "get_indexed_documents");
        let _enter = span.enter();

        debug!("Executing query: {:?}", query);
        let result = self.indexer.get_indexed_documents(query)
            .await
            .to_index_document_error()?;

        info!(
            total_count = result.total_count,
            page = result.page,
            page_size = result.page_size,
            "Documents query completed"
        );

        Ok(result)
    }

    /// Process a single document for batch indexing
    async fn process_single_document(
        command: IndexDocumentCommand,
        indexer: Arc<dyn DocumentIndexerPort>,
        analyzer: Arc<dyn TextAnalyzerPort>,
        validator: Arc<dyn DocumentValidatorPort>,
    ) -> IndexDocumentResult<DocumentIndexedResponse> {
        // Validate document
        let validation_result = validator.validate_document(&command).await?;
        if !validation_result.is_valid {
            return Err(IndexDocumentError::business_rule_validation(
                format!("Document validation failed: {:?}", validation_result.errors)
            ));
        }

        // Check for duplicates
        if !command.force_reindex {
            let is_duplicate = validator.check_duplicate_content(&command.content).await?;
            if is_duplicate {
                return Err(IndexDocumentError::business_rule_validation(
                    "Duplicate content detected".to_string()
                ));
            }
        }

        // Analyze text
        let analyze_command = AnalyzeTextCommand {
            text: command.content.clone(),
            language: command.language.clone(),
            options: Default::default(),
        };
        let analysis_result = analyzer.analyze_text(analyze_command).await?;

        // Index document
        let start_time = std::time::Instant::now();
        let mut result = indexer.index_document(command).await?;
        result.indexing_time_ms = start_time.elapsed().as_millis() as u64;
        result.token_count = analysis_result.token_count;

        Ok(result)
    }

    /// Get index health status
    #[instrument(skip(self))]
    pub async fn get_index_health(&self) -> IndexDocumentResult<super::ports::IndexHealth> {
        debug!("Getting index health status");
        let health = self.health_monitor.check_index_health()
            .await
            .to_index_document_error()?;
        Ok(health)
    }

    /// Get index statistics
    #[instrument(skip(self))]
    pub async fn get_index_stats(&self) -> IndexDocumentResult<super::ports::IndexStats> {
        debug!("Getting index statistics");
        let stats = self.health_monitor.get_index_stats()
            .await
            .to_index_document_error()?;
        Ok(stats)
    }
}

/// Builder pattern for IndexDocumentUseCase
pub struct IndexDocumentUseCaseBuilder {
    indexer: Option<Arc<dyn DocumentIndexerPort>>,
    analyzer: Option<Arc<dyn TextAnalyzerPort>>,
    health_monitor: Option<Arc<dyn IndexHealthMonitorPort>>,
    schema_manager: Option<Arc<dyn IndexSchemaManagerPort>>,
    validator: Option<Arc<dyn DocumentValidatorPort>>,
}

impl IndexDocumentUseCaseBuilder {
    pub fn new() -> Self {
        Self {
            indexer: None,
            analyzer: None,
            health_monitor: None,
            schema_manager: None,
            validator: None,
        }
    }

    pub fn indexer(mut self, indexer: Arc<dyn DocumentIndexerPort>) -> Self {
        self.indexer = Some(indexer);
        self
    }

    pub fn analyzer(mut self, analyzer: Arc<dyn TextAnalyzerPort>) -> Self {
        self.analyzer = Some(analyzer);
        self
    }

    pub fn health_monitor(mut self, health_monitor: Arc<dyn IndexHealthMonitorPort>) -> Self {
        self.health_monitor = Some(health_monitor);
        self
    }

    pub fn schema_manager(mut self, schema_manager: Arc<dyn IndexSchemaManagerPort>) -> Self {
        self.schema_manager = Some(schema_manager);
        self
    }

    pub fn validator(mut self, validator: Arc<dyn DocumentValidatorPort>) -> Self {
        self.validator = Some(validator);
        self
    }

    pub fn build(self) -> Result<IndexDocumentUseCase, String> {
        Ok(IndexDocumentUseCase {
            indexer: self.indexer.ok_or("Document indexer is required")?,
            analyzer: self.analyzer.ok_or("Text analyzer is required")?,
            health_monitor: self.health_monitor.ok_or("Health monitor is required")?,
            schema_manager: self.schema_manager.ok_or("Schema manager is required")?,
            validator: self.validator.ok_or("Document validator is required")?,
        })
    }
}

impl Default for IndexDocumentUseCaseBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use crate::features::index_text_documents::dto::*;
    use crate::features::index_text_documents::error::{IndexError, AnalysisError, TokenizationError, StemmingError, StopWordError, LanguageDetectionError, HealthError, StatsError, MetricsError, MemoryError, OptimizationError, SchemaError, ValidationError, DuplicateError};
    use crate::features::index_text_documents::ports;

    // Mock implementations for testing
    struct MockDocumentIndexer;
    
    #[async_trait]
    impl DocumentIndexerPort for MockDocumentIndexer {
        async fn index_document(&self, command: IndexDocumentCommand) -> Result<DocumentIndexedResponse, IndexError> {
            Ok(DocumentIndexedResponse {
                document_id: command.artifact_id,
                indexing_time_ms: 10,
                status: IndexingStatus::Completed,
                token_count: 5,
                operation_id: Uuid::new_v4().to_string(),
            })
        }
        
        async fn batch_index_documents(&self, _command: BatchIndexCommand) -> Result<BatchIndexResponse, IndexError> {
            Ok(BatchIndexResponse {
                results: vec![],
                batch_status: BatchOperationStatus::Completed,
                total_time_ms: 100,
                success_count: 1,
                failure_count: 0,
            })
        }
        
        async fn remove_document(&self, _command: RemoveDocumentCommand) -> Result<DocumentRemovedResponse, IndexError> {
            Ok(DocumentRemovedResponse {
                document_id: "test".to_string(),
                status: RemovalStatus::Removed,
                removal_time_ms: 5,
            })
        }
        
        async fn get_indexed_documents(&self, _query: GetIndexedDocumentsQuery) -> Result<IndexedDocumentsResponse, IndexError> {
            Ok(IndexedDocumentsResponse {
                documents: vec![],
                total_count: 0,
                page: 1,
                page_size: 20,
            })
        }
        
        async fn document_exists(&self, _document_id: &str) -> Result<bool, IndexError> {
            Ok(true)
        }
    }

    struct MockTextAnalyzer;
    
    #[async_trait]
    impl TextAnalyzerPort for MockTextAnalyzer {
        async fn analyze_text(&self, _command: AnalyzeTextCommand) -> Result<TextAnalysisResponse, AnalysisError> {
            Ok(TextAnalysisResponse {
                original_text: "test".to_string(),
                detected_language: Some("en".to_string()),
                tokens: vec![],
                token_count: 1,
                analysis_time_ms: 5,
            })
        }
        
        async fn extract_tokens(&self, _text: &str, _language: Option<&str>) -> Result<Vec<TokenInfo>, TokenizationError> {
            Ok(vec![])
        }
        
        async fn apply_stemming(&self, _tokens: Vec<TokenInfo>, _language: &str) -> Result<Vec<TokenInfo>, StemmingError> {
            Ok(vec![])
        }
        
        async fn remove_stop_words(&self, _tokens: Vec<TokenInfo>, _language: &str) -> Result<Vec<TokenInfo>, StopWordError> {
            Ok(vec![])
        }
        
        async fn detect_language(&self, _text: &str) -> Result<Option<String>, LanguageDetectionError> {
            Ok(Some("en".to_string()))
        }
    }

    struct MockHealthMonitor;
    
    #[async_trait]
    impl IndexHealthMonitorPort for MockHealthMonitor {
        async fn check_index_health(&self) -> Result<ports::IndexHealth, HealthError> {
            Ok(ports::IndexHealth {
                status: ports::HealthStatus::Healthy,
                document_count: 1000,
                index_size_bytes: 1024000,
                memory_usage_bytes: 512000,
                last_updated: Utc::now(),
                details: vec![],
            })
        }
        
        async fn get_index_stats(&self) -> Result<ports::IndexStats, StatsError> {
            Ok(ports::IndexStats {
                total_documents: 1000,
                total_tokens: 50000,
                index_size_bytes: 1024000,
                memory_usage_bytes: 512000,
                last_updated: Utc::now(),
                indexing_rate_docs_per_second: 100.0,
                query_latency_ms_p99: 25.0,
                cache_hit_rate: 0.85,
            })
        }
        
        async fn get_indexing_performance_metrics(&self, _time_range: ports::TimeRange) -> Result<ports::IndexingMetrics, MetricsError> {
            Ok(ports::IndexingMetrics {
                total_documents_indexed: 1000,
                total_indexing_time_ms: 5000,
                average_indexing_time_ms: 5.0,
                indexing_throughput_docs_per_second: 200.0,
                peak_memory_usage_bytes: 1024000,
                average_memory_usage_bytes: 512000,
                error_count: 0,
                warning_count: 2,
                last_updated: Utc::now(),
            })
        }
        
        async fn get_memory_usage(&self) -> Result<ports::MemoryUsage, MemoryError> {
            Ok(ports::MemoryUsage {
                heap_used_bytes: 512000,
                heap_total_bytes: 1024000,
                rss_bytes: 768000,
                external_bytes: 256000,
                array_buffers_bytes: 128000,
                last_updated: Utc::now(),
            })
        }
        
        async fn needs_optimization(&self) -> Result<bool, OptimizationError> {
            Ok(false)
        }
    }

    struct MockSchemaManager;
    
    #[async_trait]
    impl IndexSchemaManagerPort for MockSchemaManager {
        async fn create_schema(&self, _config: ports::SchemaConfig) -> Result<ports::SchemaInfo, SchemaError> {
            Ok(ports::SchemaInfo {
                schema_id: "test-schema".to_string(),
                name: "Test Schema".to_string(),
                version: "1.0.0".to_string(),
                fields: vec![],
                settings: ports::IndexSettings {
                    number_of_shards: 1,
                    number_of_replicas: 0,
                    refresh_interval: "30s".to_string(),
                    analysis: ports::AnalysisSettings {
                        analyzers: vec![],
                        tokenizers: vec![],
                        filters: vec![],
                    },
                },
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
        }
        
        async fn add_field(&self, _field_config: ports::FieldConfig) -> Result<ports::FieldInfo, SchemaError> {
            Ok(ports::FieldInfo {
                name: "test".to_string(),
                field_type: ports::FieldType::Text,
                indexed: true,
                stored: true,
                required: false,
                options: ports::FieldOptions {
                    tokenize: true,
                    stem: true,
                    store_term_vectors: false,
                    analyzer: None,
                },
            })
        }
        
        async fn update_field(&self, _field_name: &str, _field_config: ports::FieldConfig) -> Result<ports::FieldInfo, SchemaError> {
            self.add_field(ports::FieldConfig {
                name: "test".to_string(),
                field_type: ports::FieldType::Text,
                indexed: true,
                stored: true,
                required: false,
                options: ports::FieldOptions {
                    tokenize: true,
                    stem: true,
                    store_term_vectors: false,
                    analyzer: None,
                },
            })
        }
        
        async fn validate_schema(&self) -> Result<ports::SchemaValidationResult, SchemaError> {
            Ok(ports::SchemaValidationResult {
                is_valid: true,
                errors: vec![],
                warnings: vec![],
                recommendations: vec![],
            })
        }
    }

    struct MockDocumentValidator;
    
    #[async_trait]
    impl DocumentValidatorPort for MockDocumentValidator {
        async fn validate_document(&self, _command: &IndexDocumentCommand) -> Result<ports::ValidationResult, ValidationError> {
            Ok(ports::ValidationResult {
                is_valid: true,
                errors: vec![],
                warnings: vec![],
                score: 1.0,
            })
        }
        
        async fn validate_metadata(&self, _metadata: &ArtifactMetadata) -> Result<ports::MetadataValidationResult, ValidationError> {
            Ok(ports::MetadataValidationResult {
                is_valid: true,
                errors: vec![],
                warnings: vec![],
                recommendations: vec![],
            })
        }
        
        async fn validate_content(&self, _content: &str) -> Result<ports::ContentValidationResult, ValidationError> {
            Ok(ports::ContentValidationResult {
                is_valid: true,
                errors: vec![],
                warnings: vec![],
                content_type: "text/plain".to_string(),
                detected_language: Some("en".to_string()),
                confidence: 0.95,
            })
        }
        
        async fn check_duplicate_content(&self, _content: &str) -> Result<bool, DuplicateError> {
            Ok(false)
        }
    }

    #[tokio::test]
    async fn test_index_document_use_case() {
        let use_case = IndexDocumentUseCase::new(
            Arc::new(MockDocumentIndexer),
            Arc::new(MockTextAnalyzer),
            Arc::new(MockHealthMonitor),
            Arc::new(MockSchemaManager),
            Arc::new(MockDocumentValidator),
        );

        let command = IndexDocumentCommand::test_data();
        let result = use_case.execute(command).await.unwrap();

        assert_eq!(result.status, IndexingStatus::Completed);
        assert_eq!(result.token_count, 1);
    }

    #[tokio::test]
    async fn test_batch_index_documents() {
        let use_case = IndexDocumentUseCase::new(
            Arc::new(MockDocumentIndexer),
            Arc::new(MockTextAnalyzer),
            Arc::new(MockHealthMonitor),
            Arc::new(MockSchemaManager),
            Arc::new(MockDocumentValidator),
        );

        let command = BatchIndexCommand {
            documents: vec![IndexDocumentCommand::test_data()],
            parallel_processing: false,
            max_concurrency: None,
        };

        let result = use_case.execute_batch(command).await.unwrap();

        assert_eq!(result.batch_status, BatchOperationStatus::Completed);
        assert_eq!(result.success_count, 1);
        assert_eq!(result.failure_count, 0);
    }
}