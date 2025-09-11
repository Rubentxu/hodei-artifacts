//! API layer for Index Text Documents Feature
//!
//! This module provides HTTP endpoints and request/response handling
//! for document indexing operations following the VSA principles.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
    routing::{get, post, delete},
    Router,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, error, instrument};

use dto::*;
use error::{IndexDocumentError, WithContext};
use use_case::IndexDocumentUseCase;
use ports::{DocumentIndexerPort, TextAnalyzerPort, IndexHealthMonitorPort, IndexHealth, IndexStats};
use std::sync::Arc;

/// Application state for the index text documents feature
#[derive(Clone)]
pub struct IndexTextDocumentsState {
    pub document_use_case: Arc<IndexDocumentUseCase>,
    pub batch_use_case: Arc<IndexDocumentUseCase>,
    pub text_analyzer: Arc<dyn TextAnalyzerPort>,
    pub health_monitor: Arc<dyn IndexHealthMonitorPort>,
}

/// Query parameters for document listing
#[derive(Debug, Deserialize)]
pub struct ListDocumentsQuery {
    pub artifact_type: Option<String>,
    pub language: Option<String>,
    pub tags: Option<String>,
    pub page: Option<usize>,
    pub page_size: Option<usize>,
}

/// Error response structure
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub error_type: String,
    pub context: Option<String>,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl ErrorResponse {
    pub fn new(error: &IndexDocumentError, operation: &str) -> Self {
        Self {
            error: error.to_string(),
            error_type: std::mem::discriminant(error).to_string(),
            context: Some(operation.to_string()),
            timestamp: chrono::Utc::now(),
        }
    }
}

/// API router for index text documents feature
pub fn create_router(state: IndexTextDocumentsState) -> Router {
    Router::new()
        .route("/documents", post(index_document))
        .route("/documents/batch", post(batch_index_documents))
        .route("/documents", get(list_documents))
        .route("/documents/:id", delete(remove_document))
        .route("/documents/:id", get(get_document))
        .route("/documents/:id/exists", get(check_document_exists))
        .route("/analyze", post(analyze_text))
        .route("/health", get(get_index_health))
        .route("/stats", get(get_index_stats))
        .route("/metrics", get(get_performance_metrics))
        .with_state(state)
}

/// Index a single document
#[instrument(skip(state))]
async fn index_document(
    State(state): State<IndexTextDocumentsState>,
    Json(command): Json<IndexDocumentCommand>,
) -> Result<Json<DocumentIndexedResponse>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to index document: {}", command.artifact_id);
    
    state.document_use_case
        .execute(command)
        .await
        .with_operation_context("index_document")
        .map(|response| {
            info!("Document indexed successfully: {}", response.document_id);
            Json(response)
        })
        .map_err(|e| {
            error!("Failed to index document: {}", e);
            let status_code = match e.source {
                IndexDocumentError::DocumentValidation { .. } => StatusCode::BAD_REQUEST,
                IndexDocumentError::Indexing { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                IndexDocumentError::Configuration(_) => StatusCode::INTERNAL_SERVER_ERROR,
                IndexDocumentError::Timeout(_) => StatusCode::REQUEST_TIMEOUT,
                IndexDocumentError::ResourceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
                IndexDocumentError::BusinessRuleValidation(_) => StatusCode::BAD_REQUEST,
                IndexDocumentError::QuotaExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
                IndexDocumentError::RateLimitExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status_code,
                Json(ErrorResponse::new(&e.source, "index_document")),
            )
        })
}

/// Index multiple documents in batch
#[instrument(skip(state))]
async fn batch_index_documents(
    State(state): State<IndexTextDocumentsState>,
    Json(command): Json<BatchIndexCommand>,
) -> Result<Json<BatchIndexResponse>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to batch index {} documents", command.documents.len());
    
    state.batch_use_case
        .execute(command)
        .await
        .with_operation_context("batch_index_documents")
        .map(|response| {
            info!(
                "Batch indexing completed: {} documents processed",
                response.results.len()
            );
            Json(response)
        })
        .map_err(|e| {
            error!("Failed to batch index documents: {}", e);
            let status_code = match e.source {
                IndexDocumentError::DocumentValidation { .. } => StatusCode::BAD_REQUEST,
                IndexDocumentError::Indexing { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                IndexDocumentError::Configuration(_) => StatusCode::INTERNAL_SERVER_ERROR,
                IndexDocumentError::Timeout(_) => StatusCode::REQUEST_TIMEOUT,
                IndexDocumentError::ResourceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
                IndexDocumentError::BusinessRuleValidation(_) => StatusCode::BAD_REQUEST,
                IndexDocumentError::QuotaExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
                IndexDocumentError::RateLimitExceeded(_) => StatusCode::TOO_MANY_REQUESTS,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status_code,
                Json(ErrorResponse::new(&e.source, "batch_index_documents")),
            )
        })
}

/// List indexed documents with filters
#[instrument(skip(state))]
async fn list_documents(
    State(state): State<IndexTextDocumentsState>,
    Query(params): Query<ListDocumentsQuery>,
) -> Result<Json<IndexedDocumentsResponse>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to list documents with filters: {:?}", params);
    
    let query = GetIndexedDocumentsQuery {
        artifact_type: params.artifact_type,
        language: params.language,
        tags: params.tags.map(|tags| tags.split(',').map(|s| s.trim().to_string()).collect()),
        page: params.page,
        page_size: params.page_size,
    };
    
    state.document_use_case
        .execute_get_documents(query)
        .await
        .with_operation_context("list_documents")
        .map(|response| {
            info!("Retrieved {} documents", response.documents.len());
            Json(response)
        })
        .map_err(|e| {
            error!("Failed to list documents: {}", e);
            let status_code = match e.source {
                IndexDocumentError::Indexing { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                IndexDocumentError::Configuration(_) => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status_code,
                Json(ErrorResponse::new(&e.source, "list_documents")),
            )
        })
}

/// Remove a document from the index
#[instrument(skip(state))]
async fn remove_document(
    State(state): State<IndexTextDocumentsState>,
    Path(id): Path<String>,
) -> Result<Json<DocumentRemovedResponse>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to remove document: {}", id);
    
    let command = RemoveDocumentCommand {
        document_id: id,
        remove_metadata: true,
    };
    
    state.document_use_case
        .execute_remove_document(command)
        .await
        .with_operation_context("remove_document")
        .map(|response| {
            info!("Document removed successfully: {}", response.document_id);
            Json(response)
        })
        .map_err(|e| {
            error!("Failed to remove document: {}", e);
            let status_code = match e.source {
                IndexDocumentError::Indexing { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                IndexDocumentError::Configuration(_) => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status_code,
                Json(ErrorResponse::new(&e.source, "remove_document")),
            )
        })
}

/// Get a specific document by ID
#[instrument(skip(state))]
async fn get_document(
    State(state): State<IndexTextDocumentsState>,
    Path(id): Path<String>,
) -> Result<Json<IndexedDocumentInfo>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to get document: {}", id);
    
    state.document_use_case
        .execute_get_document(&id)
        .await
        .with_operation_context("get_document")
        .map(|document| {
            info!("Retrieved document: {}", document.document_id);
            Json(document)
        })
        .map_err(|e| {
            error!("Failed to get document: {}", e);
            let status_code = match e.source {
                IndexDocumentError::Indexing { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                IndexDocumentError::DocumentNotFound { .. } => StatusCode::NOT_FOUND,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status_code,
                Json(ErrorResponse::new(&e.source, "get_document")),
            )
        })
}

/// Check if a document exists in the index
#[instrument(skip(state))]
async fn check_document_exists(
    State(state): State<IndexTextDocumentsState>,
    Path(id): Path<String>,
) -> Result<Json<bool>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to check if document exists: {}", id);
    
    state.document_use_case
        .document_exists(&id)
        .await
        .with_operation_context("check_document_exists")
        .map(|exists| {
            info!("Document exists check: {} -> {}", id, exists);
            Json(exists)
        })
        .map_err(|e| {
            error!("Failed to check document existence: {}", e);
            let status_code = match e.source {
                IndexDocumentError::Indexing { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status_code,
                Json(ErrorResponse::new(&e.source, "check_document_exists")),
            )
        })
}

/// Analyze text content
#[instrument(skip(state))]
async fn analyze_text(
    State(state): State<IndexTextDocumentsState>,
    Json(command): Json<AnalyzeTextCommand>,
) -> Result<Json<TextAnalysisResponse>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to analyze text (length: {})", command.text.len());
    
    state.text_analyzer
        .analyze_text(command)
        .await
        .to_index_document_error()
        .with_operation_context("analyze_text")
        .map(|analysis| {
            info!("Text analysis completed for {} characters", analysis.original_length);
            Json(analysis)
        })
        .map_err(|e| {
            error!("Failed to analyze text: {}", e);
            let status_code = match e.source {
                IndexDocumentError::TextAnalysis { .. } => StatusCode::BAD_REQUEST,
                IndexDocumentError::Indexing { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status_code,
                Json(ErrorResponse::new(&e.source, "analyze_text")),
            )
        })
}

/// Get index health status
#[instrument(skip(state))]
async fn get_index_health(
    State(state): State<IndexTextDocumentsState>,
) -> Result<Json<IndexHealth>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to get index health");
    
    state.health_monitor
        .check_index_health()
        .await
        .to_index_document_error()
        .with_operation_context("get_index_health")
        .map(|health| {
            info!("Index health status: {:?}", health.status);
            Json(health)
        })
        .map_err(|e| {
            error!("Failed to get index health: {}", e);
            let status_code = match e.source {
                IndexDocumentError::HealthMonitoring { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status_code,
                Json(ErrorResponse::new(&e.source, "get_index_health")),
            )
        })
}

/// Get index statistics
#[instrument(skip(state))]
async fn get_index_stats(
    State(state): State<IndexTextDocumentsState>,
) -> Result<Json<IndexStats>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to get index stats");
    
    state.health_monitor
        .get_index_stats()
        .await
        .to_index_document_error()
        .with_operation_context("get_index_stats")
        .map(|stats| {
            info!("Index stats: {} documents, {} terms", stats.total_documents, stats.total_terms);
            Json(stats)
        })
        .map_err(|e| {
            error!("Failed to get index stats: {}", e);
            let status_code = match e.source {
                IndexDocumentError::Statistics { .. } => StatusCode::INTERNAL_SERVER_ERROR,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            (
                status_code,
                Json(ErrorResponse::new(&e.source, "get_index_stats")),
            )
        })
}

/// Get performance metrics
#[instrument(skip(state))]
async fn get_performance_metrics(
    State(state): State<IndexTextDocumentsState>,
    Query(params): Query<PerformanceMetricsQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    debug!("Received request to get performance metrics");
    
    // Placeholder implementation - return basic metrics
    let metrics = serde_json::json!({
        "total_documents_indexed": 0,
        "average_indexing_time_ms": 0.0,
        "cache_hit_rate": 0.0,
        "memory_usage_mb": 0,
        "disk_usage_mb": 0
    });
    
    // TODO: Implement proper metrics collection with TimeRange type
    // For now, return placeholder metrics
    Ok(Json(metrics))
}

/// Query parameters for performance metrics
#[derive(Debug, Deserialize)]
pub struct PerformanceMetricsQuery {
    pub start_time: String,
    pub end_time: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::index_text_documents::adapter::test::*;
    use crate::features::index_text_documents::use_case::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_index_document_success() {
        let indexer = Arc::new(MockDocumentIndexer::new());
        let use_case = IndexDocumentUseCase::new(indexer);
        
        let command = IndexDocumentCommand::test_data();
        
        let result = use_case.execute(command).await;
        assert!(result.is_ok());
        
        let response = result.unwrap();
        assert_eq!(response.status, IndexingStatus::Completed);
        assert!(response.indexing_time_ms > 0);
    }

    #[tokio::test]
    async fn test_batch_index_documents_success() {
        let indexer = Arc::new(MockDocumentIndexer::new());
        let use_case = BatchIndexUseCase::new(indexer);
        
        let command = BatchIndexCommand {
            documents: vec![
                IndexDocumentCommand::test_data(),
                IndexDocumentCommand::test_data(),
            ],
            parallel_processing: false,
            max_concurrency: None,
        };
        
        let result = use_case.execute(command).await;
        assert!(result.is_ok());
        
        let response = result.unwrap();
        assert_eq!(response.batch_status, BatchOperationStatus::Completed);
        assert_eq!(response.success_count, 2);
        assert_eq!(response.failure_count, 0);
    }
}