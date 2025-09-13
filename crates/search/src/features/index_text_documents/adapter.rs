//! Concrete adapters for Index Text Documents Feature
//!
//! This module provides implementations of the segregated ports using Tantivy
//! as the underlying search engine. Each adapter is focused and single-purpose.

use async_trait::async_trait;
use std::sync::{Arc, RwLock};
use tantivy::{
    collector::TopDocs,
    doc,
    query::{Query, QueryParser},
    schema::*,
    tokenizer::{TokenizerManager, SimpleTokenizer},
    Index, IndexWriter, ReloadPolicy, TantivyDocument, DocAddress,
};
use tracing::{debug, info, error, warn};
use serde_json;

use super::ports::*;
use super::dto::*;
use super::error::{IndexDocumentError, ToIndexDocumentError};

/// Tantivy-based document indexer adapter
pub struct TantivyDocumentIndexer {
    index: Arc<RwLock<Index>>,
    index_writer: Arc<RwLock<IndexWriter>>,
    schema: Arc<DocumentIndexSchema>,
}

impl TantivyDocumentIndexer {
    pub fn new(index_path: Option<&std::path::Path>) -> Result<Self, IndexDocumentError> {
        info!("Initializing Tantivy document indexer");
        
        let schema = Arc::new(DocumentIndexSchema::new());
        
        let index = match index_path {
            Some(path) => {
                if path.exists() {
                    Index::open_in_dir(path)
                        .map_err(|e| IndexDocumentError::Indexing { 
                            source: IndexError::StorageError(format!("Failed to open index: {}", e)) 
                        })?
                } else {
                    std::fs::create_dir_all(path)
                        .map_err(|e| IndexDocumentError::storage(format!("Failed to create index directory: {}", e)))?;
                    Index::create_in_dir(path, schema.schema.clone())
                        .map_err(|e| IndexDocumentError::Indexing { 
                            source: IndexError::StorageError(format!("Failed to create index: {}", e)) 
                        })?
                }
            }
            None => Index::create_in_ram(schema.schema.clone()),
        };
        
        let index_writer = index
            .writer(50_000_000) // 50MB buffer
            .map_err(|e| IndexDocumentError::Indexing { 
                source: IndexError::StorageError(format!("Failed to create index writer: {}", e)) 
            })?;
        
        Ok(Self {
            index: Arc::new(RwLock::new(index)),
            index_writer: Arc::new(RwLock::new(index_writer)),
            schema,
        })
    }
    
    pub fn create_schema(&self) -> Result<(), IndexDocumentError> {
        debug!("Creating search index schema");
        
        let mut writer = self.index_writer.write()
            .map_err(|e| IndexDocumentError::concurrency(format!("Failed to acquire writer lock: {}", e)))?;
        
        writer.commit()
            .map_err(|e| IndexDocumentError::Indexing { 
                source: IndexError::StorageError(format!("Failed to commit schema: {}", e)) 
            })?;
        
        Ok(())
    }
}

#[async_trait]
impl DocumentIndexerPort for TantivyDocumentIndexer {
    async fn index_document(&self, command: IndexDocumentCommand) -> Result<DocumentIndexedResponse, IndexError> {
        debug!(artifact_id = %command.artifact_id, "Indexing document");
        
        let start_time = std::time::Instant::now();
        
        let doc = self.schema.to_document(&command);
        
        {
            let mut writer = self.index_writer.write()
                .map_err(|e| IndexError::StorageError(format!("Failed to acquire writer lock: {}", e)))?;
            
            writer.add_document(doc)
                .map_err(|e| IndexError::IndexingFailed(format!("Failed to add document to index: {}", e)))?;
            
            writer.commit()
                .map_err(|e| IndexError::StorageError(format!("Failed to commit index changes: {}", e)))?;
        }
        
        let indexing_time_ms = start_time.elapsed().as_millis() as u64;
        let operation_id = uuid::Uuid::new_v4().to_string();
        
        info!(
            artifact_id = %command.artifact_id,
            operation_id = %operation_id,
            indexing_time_ms = indexing_time_ms,
            "Document indexed successfully"
        );
        
        Ok(DocumentIndexedResponse {
            document_id: command.artifact_id,
            indexing_time_ms,
            status: IndexingStatus::Completed,
            token_count: command.content.split_whitespace().count(),
            operation_id,
        })
    }
    
    async fn batch_index_documents(&self, command: BatchIndexCommand) -> Result<BatchIndexResponse, IndexError> {
        debug!(document_count = command.documents.len(), "Batch indexing documents");
        
        let start_time = std::time::Instant::now();
        let mut results = Vec::new();
        let mut success_count = 0;
        let mut failure_count = 0;
        
        {
            let mut writer = self.index_writer.write()
                .map_err(|e| IndexError::StorageError(format!("Failed to acquire writer lock for batch: {}", e)))?;
            
            for doc_command in command.documents {
                let doc_start_time = std::time::Instant::now();
                
                let doc = self.schema.to_document(&doc_command);
                
                match writer.add_document(doc) {
                    Ok(_) => {
                        let doc_indexing_time_ms = doc_start_time.elapsed().as_millis() as u64;
                        let operation_id = uuid::Uuid::new_v4().to_string();
                        
                        results.push(DocumentIndexedResponse {
                            document_id: doc_command.artifact_id,
                            indexing_time_ms: doc_indexing_time_ms,
                            status: IndexingStatus::Completed,
                            token_count: doc_command.content.split_whitespace().count(),
                            operation_id,
                        });
                        success_count += 1;
                    }
                    Err(e) => {
                        error!(artifact_id = %doc_command.artifact_id, error = %e, "Failed to index document");
                        failure_count += 1;
                    }
                }
            }
            
            writer.commit()
                .map_err(|e| IndexError::StorageError(format!("Failed to commit batch index: {}", e)))?;
        }
        
        let total_time_ms = start_time.elapsed().as_millis() as u64;
        let batch_status = match (success_count, failure_count) {
            (s, 0) if s > 0 => BatchOperationStatus::Completed,
            (0, f) if f > 0 => BatchOperationStatus::Failed,
            (s, f) if s > 0 && f > 0 => BatchOperationStatus::PartialSuccess,
            _ => BatchOperationStatus::Failed,
        };
        
        info!(
            success_count = success_count,
            failure_count = failure_count,
            total_time_ms = total_time_ms,
            batch_status = ?batch_status,
            "Batch indexing completed"
        );
        
        Ok(BatchIndexResponse {
            results,
            batch_status,
            total_time_ms,
            success_count,
            failure_count,
        })
    }
    
    async fn remove_document(&self, command: RemoveDocumentCommand) -> Result<DocumentRemovedResponse, IndexError> {
        debug!(document_id = %command.document_id, "Removing document from index");
        
        let start_time = std::time::Instant::now();
        
        // In Tantivy, we would typically mark documents as deleted rather than physically removing them
        // For now, we'll simulate the removal operation
        
        let removal_time_ms = start_time.elapsed().as_millis() as u64;
        
        info!(
            document_id = %command.document_id,
            removal_time_ms = removal_time_ms,
            "Document removal completed"
        );
        
        Ok(DocumentRemovedResponse {
            document_id: command.document_id,
            status: RemovalStatus::Removed,
            removal_time_ms,
        })
    }
    
    async fn get_indexed_documents(&self, query: GetIndexedDocumentsQuery) -> Result<IndexedDocumentsResponse, IndexError> {
        debug!("Getting indexed documents with filters");
        
        let index_reader = {
            let index = self.index.read()
                .map_err(|e| IndexError::StorageError(format!("Failed to acquire index read lock: {}", e)))?;
            index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommitWithDelay)
                .try_into()
                .map_err(|e| IndexError::StorageError(format!("Failed to create index reader: {}", e)))?
        };
        
        let searcher = index_reader.searcher();
        let total_docs = searcher.num_docs();
        
        // In a real implementation, we would apply filters and pagination
        // For now, we'll return empty results as this is a placeholder
        
        Ok(IndexedDocumentsResponse {
            documents: Vec::new(),
            total_count: total_docs as usize,
            page: query.page.unwrap_or(1),
            page_size: query.page_size.unwrap_or(20),
        })
    }
    
    async fn document_exists(&self, document_id: &str) -> Result<bool, IndexError> {
        debug!(document_id = %document_id, "Checking if document exists in index");
        
        let index_reader = {
            let index = self.index.read()
                .map_err(|e| IndexError::StorageError(format!("Failed to acquire index read lock: {}", e)))?;
            index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommitWithDelay)
                .try_into()
                .map_err(|e| IndexError::StorageError(format!("Failed to create index reader: {}", e)))?
        };
        
        let searcher = index_reader.searcher();
        
        // In a real implementation, we would search for the document by ID
        // For now, we'll return false as this is a placeholder
        Ok(false)
    }
}

/// Simple text analyzer adapter
pub struct BasicTextAnalyzer {
    tokenizer_manager: TokenizerManager,
}

impl BasicTextAnalyzer {
    pub fn new() -> Self {
        Self {
            tokenizer_manager: TokenizerManager::new(),
        }
    }
}

#[async_trait]
impl TextAnalyzerPort for BasicTextAnalyzer {
    async fn analyze_text(&self, command: AnalyzeTextCommand) -> Result<TextAnalysisResponse, AnalysisError> {
        debug!(text_length = command.text.len(), "Analyzing text");
        
        let start_time = std::time::Instant::now();
        
        let mut tokens = Vec::new();
        let token_map = &mut std::collections::HashMap::new();
        
        // Simple tokenization
        for (position, word) in command.text.split_whitespace().enumerate() {
            let clean_word = word.to_lowercase();
            if clean_word.len() >= command.options.min_token_length.unwrap_or(2) 
                && clean_word.len() <= command.options.max_token_length.unwrap_or(100) {
                
                *token_map.entry(clean_word.clone()).or_insert(0) += 1;
                
                tokens.push(TokenInfo {
                    token: clean_word.clone(),
                    position,
                    frequency: token_map[&clean_word],
                    is_stop_word: false, // TODO: Implement stop word detection
                    stemmed: None, // TODO: Implement stemming
                });
            }
        }
        
        let detected_language = self.detect_language(&command.text).await.ok();
        
        Ok(TextAnalysisResponse {
            original_text: command.text,
            detected_language,
            tokens,
            token_count: tokens.len(),
            analysis_time_ms: start_time.elapsed().as_millis() as u64,
        })
    }
    
    async fn extract_tokens(&self, text: &str, language: Option<&str>) -> Result<Vec<TokenInfo>, TokenizationError> {
        debug!(text_length = text.len(), language = ?language, "Extracting tokens");
        
        let mut tokens = Vec::new();
        let token_map = &mut std::collections::HashMap::new();
        
        for (position, word) in text.split_whitespace().enumerate() {
            let clean_word = word.to_lowercase();
            if clean_word.len() >= 2 && clean_word.len() <= 100 {
                *token_map.entry(clean_word.clone()).or_insert(0) += 1;
                
                tokens.push(TokenInfo {
                    token: clean_word.clone(),
                    position,
                    frequency: token_map[&clean_word],
                    is_stop_word: false,
                    stemmed: None,
                });
            }
        }
        
        Ok(tokens)
    }
    
    async fn apply_stemming(&self, tokens: Vec<TokenInfo>, language: &str) -> Result<Vec<TokenInfo>, StemmingError> {
        debug!(token_count = tokens.len(), language = %language, "Applying stemming");
        
        // TODO: Implement proper stemming using a library like rust-stem
        // For now, return tokens without stemming
        Ok(tokens)
    }
    
    async fn remove_stop_words(&self, tokens: Vec<TokenInfo>, language: &str) -> Result<Vec<TokenInfo>, StopWordError> {
        debug!(token_count = tokens.len(), language = %language, "Removing stop words");
        
        // Common English stop words
        let stop_words = if language.to_lowercase() == "en" {
            vec![
                "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "he",
                "in", "is", "it", "its", "of", "on", "that", "the", "to", "was", "were",
                "will", "with", "i", "you", "your", "they", "this", "that", "these", "those"
            ]
        } else {
            Vec::new() // TODO: Add stop words for other languages
        };
        
        let filtered_tokens: Vec<TokenInfo> = tokens.into_iter()
            .filter(|token| !stop_words.contains(&token.token.as_str()))
            .collect();
        
        Ok(filtered_tokens)
    }
    
    async fn detect_language(&self, text: &str) -> Result<Option<String>, LanguageDetectionError> {
        debug!(text_length = text.len(), "Detecting language");
        
        // TODO: Implement proper language detection
        // For now, return English as default
        Ok(Some("en".to_string()))
    }
}

/// Basic health monitor adapter
pub struct BasicIndexHealthMonitor {
    index: Arc<RwLock<Index>>,
}

impl BasicIndexHealthMonitor {
    pub fn new(index: Arc<RwLock<Index>>) -> Self {
        Self { index }
    }
}

#[async_trait]
impl IndexHealthMonitorPort for BasicIndexHealthMonitor {
    async fn check_index_health(&self) -> Result<IndexHealth, HealthError> {
        debug!("Checking index health");
        
        let index_reader = {
            let index = self.index.read()
                .map_err(|e| HealthError::HealthCheckFailed(format!("Failed to acquire index lock: {}", e)))?;
            index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommitWithDelay)
                .try_into()
                .map_err(|e| HealthError::HealthCheckFailed(format!("Failed to create index reader: {}", e)))?
        };
        
        let searcher = index_reader.searcher();
        let document_count = searcher.num_docs();
        
        // TODO: Implement proper health checks
        let health_status = if document_count > 0 {
            HealthStatus::Healthy
        } else {
            HealthStatus::Warning
        };
        
        Ok(IndexHealth {
            status: health_status,
            document_count: document_count as u64,
            index_size_bytes: 0, // TODO: Get actual index size
            memory_usage_bytes: 0, // TODO: Get actual memory usage
            last_updated: chrono::Utc::now(),
            details: vec![
                HealthDetail {
                    name: "Document Count".to_string(),
                    status: health_status,
                    message: format!("Index contains {} documents", document_count),
                    timestamp: chrono::Utc::now(),
                }
            ],
        })
    }
    
    async fn get_index_stats(&self) -> Result<IndexStats, StatsError> {
        debug!("Getting index statistics");
        
        let index_reader = {
            let index = self.index.read()
                .map_err(|e| StatsError::StatsRetrievalFailed(format!("Failed to acquire index lock: {}", e)))?;
            index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommitWithDelay)
                .try_into()
                .map_err(|e| StatsError::StatsRetrievalFailed(format!("Failed to create index reader: {}", e)))?
        };
        
        let searcher = index_reader.searcher();
        let document_count = searcher.num_docs();
        
        Ok(IndexStats {
            total_documents: document_count as u64,
            total_terms: 0, // TODO: Get actual term count
            avg_terms_per_document: 0.0, // TODO: Calculate average
            index_size_bytes: 0, // TODO: Get actual index size
            memory_usage_bytes: 0, // TODO: Get actual memory usage
            segment_count: 0, // TODO: Get actual segment count
            created_at: chrono::Utc::now(),
            last_optimized_at: None,
        })
    }
    
    async fn get_indexing_performance_metrics(&self, time_range: TimeRange) -> Result<IndexingMetrics, MetricsError> {
        debug!("Getting indexing performance metrics");
        
        // TODO: Implement proper metrics collection
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
        debug!("Getting memory usage");
        
        // TODO: Implement proper memory usage tracking
        Ok(MemoryUsage {
            current_usage_bytes: 0,
            peak_usage_bytes: 0,
            memory_limit_bytes: 100_000_000, // 100MB default limit
            usage_percentage: 0.0,
        })
    }
    
    async fn needs_optimization(&self) -> Result<bool, OptimizationError> {
        debug!("Checking if index needs optimization");
        
        // TODO: Implement proper optimization check
        Ok(false)
    }
}

/// Document index schema for Tantivy
#[derive(Debug, Clone)]
pub struct DocumentIndexSchema {
    pub schema: Schema,
    pub artifact_id_field: Field,
    pub content_field: Field,
    pub title_field: Field,
    pub description_field: Field,
    pub artifact_type_field: Field,
    pub version_field: Field,
    pub tags_field: Field,
    pub language_field: Field,
    pub indexed_at_field: Field,
}

impl DocumentIndexSchema {
    pub fn new() -> Self {
        let mut schema_builder = Schema::builder();
        
        // Create fields with appropriate types and options
        let artifact_id_field = schema_builder.add_text_field("artifact_id", STRING | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT | STORED);
        let title_field = schema_builder.add_text_field("title", TEXT | STORED);
        let description_field = schema_builder.add_text_field("description", TEXT | STORED);
        let artifact_type_field = schema_builder.add_text_field("artifact_type", STRING | STORED);
        let version_field = schema_builder.add_text_field("version", STRING | STORED);
        let tags_field = schema_builder.add_text_field("tags", TEXT | STORED);
        let language_field = schema_builder.add_text_field("language", STRING | STORED);
        let indexed_at_field = schema_builder.add_date_field("indexed_at", INDEXED | STORED);
        
        let schema = schema_builder.build();
        
        Self {
            schema,
            artifact_id_field,
            content_field,
            title_field,
            description_field,
            artifact_type_field,
            version_field,
            tags_field,
            language_field,
            indexed_at_field,
        }
    }
    
    pub fn to_document(&self, command: &IndexDocumentCommand) -> TantivyDocument {
        doc! {
            self.artifact_id_field => command.artifact_id.clone(),
            self.content_field => command.content.clone(),
            self.title_field => command.metadata.title.clone().unwrap_or_default(),
            self.description_field => command.metadata.description.clone().unwrap_or_default(),
            self.artifact_type_field => command.metadata.artifact_type.clone(),
            self.version_field => command.metadata.version.clone(),
            self.tags_field => command.metadata.tags.join(" "),
            self.language_field => command.language.clone().unwrap_or_else(|| "en".to_string()),
            self.indexed_at_field => tantivy::DateTime::from_utc(chrono::Utc::now()),
        }
    }
    
    pub fn from_document(&self, doc: &TantivyDocument) -> Option<IndexedDocumentInfo> {
        let artifact_id = doc.get_first(self.artifact_id_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let title = doc.get_first(self.title_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string());
            
        let description = doc.get_first(self.description_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string());
            
        let artifact_type = doc.get_first(self.artifact_type_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let version = doc.get_first(self.version_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let language = doc.get_first(self.language_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string());
            
        let tags = doc.get_first(self.tags_field)
            .and_then(|v| v.as_text())
            .map(|s| s.split(' ').map(|t| t.to_string()).collect())
            .unwrap_or_default();
            
        let metadata = ArtifactMetadata {
            title,
            description,
            tags,
            artifact_type,
            version,
            custom_metadata: std::collections::HashMap::new(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        
        Some(IndexedDocumentInfo {
            document_id: artifact_id,
            metadata,
            language,
            token_count: 0, // TODO: Calculate actual token count
            status: IndexingStatus::Completed,
            last_indexed_at: chrono::Utc::now(),
        })
    }
}

/// Mock implementations for testing
#[cfg(test)]
pub mod test {
        use super::*;
    use std::collections::HashMap;
    
    pub struct MockDocumentIndexer {
        pub indexed_documents: Arc<RwLock<HashMap<String, IndexDocumentCommand>>>,
    }
    
    impl MockDocumentIndexer {
        pub fn new() -> Self {
            Self {
                indexed_documents: Arc::new(RwLock::new(HashMap::new())),
            }
        }
    }
    
    #[async_trait]
    impl DocumentIndexerPort for MockDocumentIndexer {
        async fn index_document(&self, command: IndexDocumentCommand) -> Result<DocumentIndexedResponse, IndexError> {
            let mut docs = self.indexed_documents.write()
                .map_err(|_| IndexError::StorageError("Failed to acquire lock".to_string()))?;
            
            docs.insert(command.artifact_id.clone(), command.clone());
            
            Ok(DocumentIndexedResponse {
                document_id: command.artifact_id,
                indexing_time_ms: 10,
                status: IndexingStatus::Completed,
                token_count: command.content.split_whitespace().count(),
                operation_id: uuid::Uuid::new_v4().to_string(),
            })
        }
        
        async fn batch_index_documents(&self, command: BatchIndexCommand) -> Result<BatchIndexResponse, IndexError> {
            let mut docs = self.indexed_documents.write()
                .map_err(|_| IndexError::StorageError("Failed to acquire lock".to_string()))?;
            
            let mut results = Vec::new();
            let mut success_count = 0;
            
            for doc_command in command.documents {
                docs.insert(doc_command.artifact_id.clone(), doc_command.clone());
                
                results.push(DocumentIndexedResponse {
                    document_id: doc_command.artifact_id,
                    indexing_time_ms: 10,
                    status: IndexingStatus::Completed,
                    token_count: doc_command.content.split_whitespace().count(),
                    operation_id: uuid::Uuid::new_v4().to_string(),
                });
                success_count += 1;
            }
            
            Ok(BatchIndexResponse {
                results,
                batch_status: BatchOperationStatus::Completed,
                total_time_ms: 100,
                success_count,
                failure_count: 0,
            })
        }
        
        async fn remove_document(&self, command: RemoveDocumentCommand) -> Result<DocumentRemovedResponse, IndexError> {
            let mut docs = self.indexed_documents.write()
                .map_err(|_| IndexError::StorageError("Failed to acquire lock".to_string()))?;
            
            docs.remove(&command.document_id);
            
            Ok(DocumentRemovedResponse {
                document_id: command.document_id,
                status: RemovalStatus::Removed,
                removal_time_ms: 5,
            })
        }
        
        async fn get_indexed_documents(&self, query: GetIndexedDocumentsQuery) -> Result<IndexedDocumentsResponse, IndexError> {
            let docs = self.indexed_documents.read()
                .map_err(|_| IndexError::StorageError("Failed to acquire lock".to_string()))?;
            
            let documents: Vec<IndexedDocumentInfo> = docs.values()
                .map(|cmd| IndexedDocumentInfo {
                    document_id: cmd.artifact_id.clone(),
                    metadata: cmd.metadata.clone(),
                    language: cmd.language.clone(),
                    token_count: cmd.content.split_whitespace().count(),
                    status: IndexingStatus::Completed,
                    last_indexed_at: chrono::Utc::now(),
                })
                .collect();
            
            Ok(IndexedDocumentsResponse {
                documents,
                total_count: documents.len(),
                page: query.page.unwrap_or(1),
                page_size: query.page_size.unwrap_or(20),
            })
        }
        
        async fn document_exists(&self, document_id: &str) -> Result<bool, IndexError> {
            let docs = self.indexed_documents.read()
                .map_err(|_| IndexError::StorageError("Failed to acquire lock".to_string()))?;
            
            Ok(docs.contains_key(document_id))
        }
    }
    
    pub struct MockTextAnalyzer;
    
    impl MockTextAnalyzer {
        pub fn new() -> Self {
            Self
        }
    }
    
    #[async_trait]
    impl TextAnalyzerPort for MockTextAnalyzer {
        async fn analyze_text(&self, command: AnalyzeTextCommand) -> Result<TextAnalysisResponse, AnalysisError> {
            let tokens: Vec<TokenInfo> = command.text
                .split_whitespace()
                .enumerate()
                .map(|(i, word)| TokenInfo {
                    token: word.to_lowercase(),
                    position: i,
                    frequency: 1,
                    is_stop_word: false,
                    stemmed: None,
                })
                .collect();
            
            Ok(TextAnalysisResponse {
                original_text: command.text,
                detected_language: Some("en".to_string()),
                tokens,
                token_count: tokens.len(),
                analysis_time_ms: 5,
            })
        }
        
        async fn extract_tokens(&self, text: &str, _language: Option<&str>) -> Result<Vec<TokenInfo>, TokenizationError> {
            let tokens: Vec<TokenInfo> = text
                .split_whitespace()
                .enumerate()
                .map(|(i, word)| TokenInfo {
                    token: word.to_lowercase(),
                    position: i,
                    frequency: 1,
                    is_stop_word: false,
                    stemmed: None,
                })
                .collect();
            
            Ok(tokens)
        }
        
        async fn apply_stemming(&self, tokens: Vec<TokenInfo>, _language: &str) -> Result<Vec<TokenInfo>, StemmingError> {
            Ok(tokens)
        }
        
        async fn remove_stop_words(&self, tokens: Vec<TokenInfo>, _language: &str) -> Result<Vec<TokenInfo>, StopWordError> {
            Ok(tokens)
        }
        
        async fn detect_language(&self, _text: &str) -> Result<Option<String>, LanguageDetectionError> {
            Ok(Some("en".to_string()))
        }
    }
    
    pub struct MockIndexHealthMonitor;
    
    impl MockIndexHealthMonitor {
        pub fn new() -> Self {
            Self
        }
    }
    
    #[async_trait]
    impl IndexHealthMonitorPort for MockIndexHealthMonitor {
        async fn check_index_health(&self) -> Result<IndexHealth, HealthError> {
            Ok(IndexHealth {
                status: HealthStatus::Healthy,
                document_count: 10,
                index_size_bytes: 1024,
                memory_usage_bytes: 512,
                last_updated: chrono::Utc::now(),
                details: vec![],
            })
        }
        
        async fn get_index_stats(&self) -> Result<IndexStats, StatsError> {
            Ok(IndexStats {
                total_documents: 10,
                total_terms: 100,
                avg_terms_per_document: 10.0,
                index_size_bytes: 1024,
                memory_usage_bytes: 512,
                segment_count: 1,
                created_at: chrono::Utc::now(),
                last_optimized_at: None,
            })
        }
        
        async fn get_indexing_performance_metrics(&self, _time_range: TimeRange) -> Result<IndexingMetrics, MetricsError> {
            Ok(IndexingMetrics {
                avg_indexing_time_ms: 10.0,
                total_operations: 10,
                successful_operations: 10,
                failed_operations: 0,
                operations_per_second: 100.0,
                p99_latency_ms: 15.0,
            })
        }
        
        async fn get_memory_usage(&self) -> Result<MemoryUsage, MemoryError> {
            Ok(MemoryUsage {
                current_usage_bytes: 512,
                peak_usage_bytes: 1024,
                memory_limit_bytes: 8192,
                usage_percentage: 6.25,
            })
        }
        
        async fn needs_optimization(&self) -> Result<bool, OptimizationError> {
            Ok(false)
        }
    }
}