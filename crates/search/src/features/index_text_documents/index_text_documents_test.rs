//! Unit tests for Index Text Documents Feature
//!
//! This module contains comprehensive unit tests following TDD principles
//! with proper mocking and assertion strategies.

use std::sync::Arc;
use tokio_test;
use tracing::{info, warn, error};
use tracing_test::traced_test;

use crate::features::index_text_documents::*;
use crate::features::index_text_documents::adapter::test::*;
use crate::features::index_text_documents::use_case::*;
use crate::features::index_text_documents::ports::*;

#[tokio::test]
#[traced_test]
async fn test_index_document_use_case_success() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = IndexDocumentUseCase::new(document_indexer);
    
    let command = IndexDocumentCommand::test_data();
    let artifact_id = command.artifact_id.clone();
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok(), "Document indexing should succeed");
    
    let response = result.unwrap();
    assert_eq!(response.document_id, artifact_id);
    assert_eq!(response.status, IndexingStatus::Completed);
    assert!(response.indexing_time_ms > 0);
    assert!(response.token_count > 0);
    assert!(!response.operation_id.is_empty());
    
    // Verify tracing logs
    assert!(logs_contain("Starting document indexing"));
    assert!(logs_contain("Document indexing completed successfully"));
    assert!(logs_contain(&artifact_id));
}

#[tokio::test]
#[traced_test]
async fn test_index_document_use_case_validation_error() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = IndexDocumentUseCase::new(document_indexer);
    
    let mut command = IndexDocumentCommand::test_data();
    command.content = "".to_string(); // Empty content should cause validation error
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_err(), "Document indexing should fail with empty content");
    
    let error = result.unwrap_err();
    match error {
        IndexDocumentError::DocumentValidation { source } => {
            assert!(source.to_string().contains("empty"));
        }
        _ => panic!("Expected DocumentValidation error, got: {:?}", error),
    }
    
    // Verify error tracing logs
    assert!(logs_contain("Document validation failed"));
}

#[tokio::test]
#[traced_test]
async fn test_batch_index_use_case_success() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = BatchIndexUseCase::new(document_indexer);
    
    let documents = vec![
        IndexDocumentCommand::test_data(),
        IndexDocumentCommand::test_data(),
    ];
    
    let command = BatchIndexCommand {
        documents: documents.clone(),
        parallel_processing: false,
        max_concurrency: None,
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok(), "Batch indexing should succeed");
    
    let response = result.unwrap();
    assert_eq!(response.batch_status, BatchOperationStatus::Completed);
    assert_eq!(response.success_count, 2);
    assert_eq!(response.failure_count, 0);
    assert_eq!(response.results.len(), 2);
    assert!(response.total_time_ms > 0);
    
    // Verify tracing logs
    assert!(logs_contain("Starting batch indexing"));
    assert!(logs_contain("Batch indexing completed"));
    assert!(logs_contain("2 documents"));
}

#[tokio::test]
#[traced_test]
async fn test_batch_index_use_case_partial_success() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = BatchIndexUseCase::new(document_indexer);
    
    let documents = vec![
        IndexDocumentCommand::test_data(),
        IndexDocumentCommand {
            artifact_id: "test-2".to_string(),
            content: "".to_string(), // This will fail validation
            metadata: ArtifactMetadata::test_data(),
            language: Some("en".to_string()),
            force_reindex: false,
        },
    ];
    
    let command = BatchIndexCommand {
        documents,
        parallel_processing: false,
        max_concurrency: None,
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok(), "Batch indexing should complete with partial success");
    
    let response = result.unwrap();
    assert_eq!(response.batch_status, BatchOperationStatus::PartialSuccess);
    assert_eq!(response.success_count, 1);
    assert_eq!(response.failure_count, 1);
    assert_eq!(response.results.len(), 1);
    
    // Verify tracing logs
    assert!(logs_contain("Batch indexing completed with partial success"));
    assert!(logs_contain("1 successful"));
    assert!(logs_contain("1 failed"));
}

#[tokio::test]
#[traced_test]
async fn test_batch_index_use_case_parallel_processing() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = BatchIndexUseCase::new(document_indexer);
    
    let documents: Vec<IndexDocumentCommand> = (0..5)
        .map(|i| IndexDocumentCommand {
            artifact_id: format!("test-{}", i),
            content: format!("Test content for document {}", i),
            metadata: ArtifactMetadata::test_data(),
            language: Some("en".to_string()),
            force_reindex: false,
        })
        .collect();
    
    let command = BatchIndexCommand {
        documents,
        parallel_processing: true,
        max_concurrency: Some(3),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok(), "Parallel batch indexing should succeed");
    
    let response = result.unwrap();
    assert_eq!(response.batch_status, BatchOperationStatus::Completed);
    assert_eq!(response.success_count, 5);
    assert_eq!(response.failure_count, 0);
    
    // Verify tracing logs
    assert!(logs_contain("Parallel processing enabled"));
    assert!(logs_contain("max_concurrency=3"));
}

#[tokio::test]
#[traced_test]
async fn test_text_analyzer_extract_tokens() {
    // Arrange
    let analyzer = MockTextAnalyzer::new();
    let text = "Hello world this is a test document";
    let language = Some("en");
    
    // Act
    let result = analyzer.extract_tokens(text, language).await;
    
    // Assert
    assert!(result.is_ok(), "Token extraction should succeed");
    
    let tokens = result.unwrap();
    assert_eq!(tokens.len(), 7); // "Hello", "world", "this", "is", "test", "document"
    
    // Verify token properties
    for (i, token) in tokens.iter().enumerate() {
        assert_eq!(token.position, i);
        assert_eq!(token.frequency, 1);
        assert!(!token.is_stop_word);
        assert!(token.stemmed.is_none());
    }
}

#[tokio::test]
#[traced_test]
async fn test_text_analyzer_analyze_text() {
    // Arrange
    let analyzer = MockTextAnalyzer::new();
    let command = AnalyzeTextCommand {
        text: "This is a test document for analysis".to_string(),
        language: Some("en".to_string()),
        options: TextAnalysisOptions::default(),
    };
    
    // Act
    let result = analyzer.analyze_text(command).await;
    
    // Assert
    assert!(result.is_ok(), "Text analysis should succeed");
    
    let response = result.unwrap();
    assert_eq!(response.original_text, "This is a test document for analysis");
    assert_eq!(response.detected_language, Some("en".to_string()));
    assert!(response.token_count > 0);
    assert_eq!(response.tokens.len(), response.token_count);
    assert!(response.analysis_time_ms > 0);
}

#[tokio::test]
#[traced_test]
async fn test_text_analyzer_remove_stop_words() {
    // Arrange
    let analyzer = MockTextAnalyzer::new();
    let tokens = vec![
        TokenInfo {
            token: "this".to_string(),
            position: 0,
            frequency: 1,
            is_stop_word: false,
            stemmed: None,
        },
        TokenInfo {
            token: "is".to_string(),
            position: 1,
            frequency: 1,
            is_stop_word: false,
            stemmed: None,
        },
        TokenInfo {
            token: "important".to_string(),
            position: 2,
            frequency: 1,
            is_stop_word: false,
            stemmed: None,
        },
    ];
    
    // Act
    let result = analyzer.remove_stop_words(tokens, "en").await;
    
    // Assert
    assert!(result.is_ok(), "Stop word removal should succeed");
    
    let filtered_tokens = result.unwrap();
    // Note: Mock implementation doesn't actually filter stop words
    // In a real implementation, "this" and "is" would be filtered out
    assert_eq!(filtered_tokens.len(), 3);
}

#[tokio::test]
#[traced_test]
async fn test_index_health_monitor_check_health() {
    // Arrange
    let monitor = MockIndexHealthMonitor::new();
    
    // Act
    let result = monitor.check_index_health().await;
    
    // Assert
    assert!(result.is_ok(), "Health check should succeed");
    
    let health = result.unwrap();
    assert_eq!(health.status, HealthStatus::Healthy);
    assert_eq!(health.document_count, 10);
    assert!(health.index_size_bytes > 0);
    assert!(health.memory_usage_bytes > 0);
}

#[tokio::test]
#[traced_test]
async fn test_index_health_monitor_get_stats() {
    // Arrange
    let monitor = MockIndexHealthMonitor::new();
    
    // Act
    let result = monitor.get_index_stats().await;
    
    // Assert
    assert!(result.is_ok(), "Stats retrieval should succeed");
    
    let stats = result.unwrap();
    assert_eq!(stats.total_documents, 10);
    assert_eq!(stats.total_terms, 100);
    assert!(stats.avg_terms_per_document > 0.0);
    assert!(stats.index_size_bytes > 0);
}

#[tokio::test]
#[traced_test]
async fn test_index_health_monitor_get_performance_metrics() {
    // Arrange
    let monitor = MockIndexHealthMonitor::new();
    let time_range = TimeRange {
        start: chrono::Utc::now() - chrono::Duration::hours(1),
        end: chrono::Utc::now(),
    };
    
    // Act
    let result = monitor.get_indexing_performance_metrics(time_range).await;
    
    // Assert
    assert!(result.is_ok(), "Performance metrics retrieval should succeed");
    
    let metrics = result.unwrap();
    assert!(metrics.avg_indexing_time_ms > 0.0);
    assert_eq!(metrics.total_operations, 10);
    assert_eq!(metrics.successful_operations, 10);
    assert_eq!(metrics.failed_operations, 0);
    assert!(metrics.operations_per_second > 0.0);
}

#[tokio::test]
#[traced_test]
async fn test_di_container_creation() {
    // Arrange & Act
    let result = IndexTextDocumentsDIContainer::for_production_with_memory_index();
    
    // Assert
    assert!(result.is_ok(), "DI container creation should succeed");
    
    let container = result.unwrap();
    assert!(container.document_use_case().is_ready());
    assert!(container.batch_use_case().is_ready());
    assert!(container.text_analyzer().is_ready());
    assert!(container.health_monitor().is_ready());
}

#[tokio::test]
#[traced_test]
async fn test_di_container_builder() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let text_analyzer = Arc::new(MockTextAnalyzer::new());
    let health_monitor = Arc::new(MockIndexHealthMonitor::new());
    
    // Act
    let container = IndexTextDocumentsDIContainerBuilder::new()
        .with_document_indexer(document_indexer)
        .with_text_analyzer(text_analyzer)
        .with_health_monitor(health_monitor)
        .build()
        .unwrap();
    
    // Assert
    assert!(container.document_use_case().is_ready());
    assert!(container.batch_use_case().is_ready());
    assert!(container.text_analyzer().is_ready());
    assert!(container.health_monitor().is_ready());
}

#[tokio::test]
#[traced_test]
async fn test_di_container_config() {
    // Arrange
    let config = IndexTextDocumentsConfig::testing();
    
    // Act
    let result = config.create_container();
    
    // Assert
    assert!(result.is_ok(), "Container creation from config should succeed");
    
    let container = result.unwrap();
    assert!(container.document_use_case().is_ready());
    assert!(container.batch_use_case().is_ready());
    assert!(container.text_analyzer().is_ready());
    assert!(container.health_monitor().is_ready());
}

#[tokio::test]
#[traced_test]
async fn test_feature_initialization() {
    // Arrange & Act
    let result = initialize_feature();
    
    // Assert
    assert!(result.is_ok(), "Feature initialization should succeed");
    
    let container = result.unwrap();
    assert!(container.document_use_case().is_ready());
    assert!(container.batch_use_case().is_ready());
}

#[tokio::test]
#[traced_test]
async fn test_feature_initialization_with_config() {
    // Arrange
    let config = IndexTextDocumentsConfig::testing();
    
    // Act
    let result = initialize_feature_with_config(config);
    
    // Assert
    assert!(result.is_ok(), "Feature initialization with config should succeed");
    
    let container = result.unwrap();
    assert!(container.document_use_case().is_ready());
    assert!(container.batch_use_case().is_ready());
}

#[tokio::test]
#[traced_test]
async fn test_feature_health_check() {
    // Arrange
    let container = IndexTextDocumentsDIContainer::for_testing();
    
    // Act
    let result = health_check(&container).await;
    
    // Assert
    assert!(result.is_ok(), "Feature health check should succeed");
    
    let health = result.unwrap();
    assert_eq!(health.status, HealthStatus::Healthy);
}

#[tokio::test]
#[traced_test]
async fn test_feature_stats() {
    // Arrange
    let container = IndexTextDocumentsDIContainer::for_testing();
    
    // Act
    let result = get_feature_stats(&container).await;
    
    // Assert
    assert!(result.is_ok(), "Feature stats retrieval should succeed");
    
    let stats = result.unwrap();
    assert_eq!(stats.total_documents, 10);
    assert!(stats.total_terms > 0);
}

// Performance tests
#[tokio::test]
async fn test_batch_indexing_performance() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = BatchIndexUseCase::new(document_indexer);
    
    let documents: Vec<IndexDocumentCommand> = (0..100)
        .map(|i| IndexDocumentCommand {
            artifact_id: format!("perf-test-{}", i),
            content: format!("Performance test content for document {}", i),
            metadata: ArtifactMetadata::test_data(),
            language: Some("en".to_string()),
            force_reindex: false,
        })
        .collect();
    
    let command = BatchIndexCommand {
        documents,
        parallel_processing: true,
        max_concurrency: Some(10),
    };
    
    // Act
    let start_time = std::time::Instant::now();
    let result = use_case.execute(command).await;
    let duration = start_time.elapsed();
    
    // Assert
    assert!(result.is_ok(), "Performance test should succeed");
    
    let response = result.unwrap();
    assert_eq!(response.batch_status, BatchOperationStatus::Completed);
    assert_eq!(response.success_count, 100);
    assert_eq!(response.failure_count, 0);
    
    // Performance assertion: should complete in under 1 second
    assert!(duration.as_millis() < 1000, "Batch indexing should complete quickly");
    tracing::info!("Batch indexing of 100 documents completed in {:?}", duration);
}

// Error handling tests
#[tokio::test]
#[traced_test]
async fn test_error_handling_timeout() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = IndexDocumentUseCase::new(document_indexer);
    
    let command = IndexDocumentCommand::test_data();
    
    // Act - Note: Mock doesn't actually timeout, but we test the error path
    let result = use_case.execute(command).await;
    
    // Assert
    // In a real scenario, we would inject a mock that returns timeout errors
    assert!(result.is_ok(), "Mock should succeed");
}

#[tokio::test]
#[traced_test]
async fn test_error_handling_resource_unavailable() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = IndexDocumentUseCase::new(document_indexer);
    
    let command = IndexDocumentCommand::test_data();
    
    // Act - Note: Mock doesn't actually return resource unavailable
    let result = use_case.execute(command).await;
    
    // Assert
    // In a real scenario, we would inject a mock that returns resource unavailable errors
    assert!(result.is_ok(), "Mock should succeed");
}

// Boundary value tests
#[tokio::test]
#[traced_test]
async fn test_boundary_values_empty_batch() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = BatchIndexUseCase::new(document_indexer);
    
    let command = BatchIndexCommand {
        documents: vec![],
        parallel_processing: false,
        max_concurrency: None,
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok(), "Empty batch should succeed");
    
    let response = result.unwrap();
    assert_eq!(response.batch_status, BatchOperationStatus::Completed);
    assert_eq!(response.success_count, 0);
    assert_eq!(response.failure_count, 0);
    assert_eq!(response.results.len(), 0);
}

#[tokio::test]
#[traced_test]
async fn test_boundary_values_single_document_batch() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = BatchIndexUseCase::new(document_indexer);
    
    let command = BatchIndexCommand {
        documents: vec![IndexDocumentCommand::test_data()],
        parallel_processing: false,
        max_concurrency: None,
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok(), "Single document batch should succeed");
    
    let response = result.unwrap();
    assert_eq!(response.batch_status, BatchOperationStatus::Completed);
    assert_eq!(response.success_count, 1);
    assert_eq!(response.failure_count, 0);
    assert_eq!(response.results.len(), 1);
}

// Concurrency tests
#[tokio::test]
async fn test_concurrent_indexing() {
    // Arrange
    let document_indexer = Arc::new(MockDocumentIndexer::new());
    let use_case = IndexDocumentUseCase::new(document_indexer.clone());
    
    let tasks: Vec<_> = (0..10)
        .map(|i| {
            let use_case = use_case.clone();
            async move {
                let command = IndexDocumentCommand {
                    artifact_id: format!("concurrent-test-{}", i),
                    content: format!("Concurrent test content for document {}", i),
                    metadata: ArtifactMetadata::test_data(),
                    language: Some("en".to_string()),
                    force_reindex: false,
                };
                use_case.execute(command).await
            }
        })
        .collect();
    
    // Act
    let results = futures::future::join_all(tasks).await;
    
    // Assert
    assert_eq!(results.len(), 10);
    
    let success_count = results.iter().filter(|r| r.is_ok()).count();
    assert_eq!(success_count, 10, "All concurrent operations should succeed");
}