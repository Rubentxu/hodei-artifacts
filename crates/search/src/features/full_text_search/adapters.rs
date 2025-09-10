use async_trait::async_trait;
use std::sync::{Arc, RwLock};
use tantivy::{
    collector::TopDocs,
    query::{Query, QueryParser},
    schema::*,
    tokenizer::{TokenizerManager, SimpleTokenizer},
    Index, IndexWriter, ReloadPolicy, TantivyDocument,
};
use tracing::{debug, info, error};

use crate::features::full_text_search::{
    ports::{
        SearchEnginePort, IndexerPort, TokenizerPort, ScorerPort,
        SearchStats, BatchIndexingResult, ReindexingResult, Token,
    },
    dto::{FullTextSearchQuery, FullTextSearchResults, IndexedArtifact, ScoredArtifact, Highlight},
    error::FullTextSearchError,
};

/// Tantivy-based search engine adapter
pub struct TantivySearchEngineAdapter {
    index: Arc<RwLock<Index>>,
    index_writer: Arc<RwLock<IndexWriter>>,
    schema: SearchSchema,
    tokenizer_manager: TokenizerManager,
}

impl TantivySearchEngineAdapter {
    pub fn new() -> Result<Self, FullTextSearchError> {
        info!("Initializing Tantivy search engine adapter");
        
        let schema = SearchSchema::new();
        let index = Index::create_in_ram(schema.schema.clone());
        
        // Configure tokenizer manager
        let tokenizer_manager = index.tokenizers().clone();
        
        let index_writer = index
            .writer(50_000_000) // 50MB buffer
            .map_err(|e| FullTextSearchError::InternalError(format!("Failed to create index writer: {}", e)))?;
        
        Ok(Self {
            index: Arc::new(RwLock::new(index)),
            index_writer: Arc::new(RwLock::new(index_writer)),
            schema,
            tokenizer_manager,
        })
    }
}

#[async_trait]
impl SearchEnginePort for TantivySearchEngineAdapter {
    async fn search(
        &self,
        query: &FullTextSearchQuery,
    ) -> Result<FullTextSearchResults, FullTextSearchError> {
        debug!(query = %query.q, "Searching in Tantivy");
        
        let start_time = std::time::Instant::now();
        
        let index_reader = {
            let index = self.index.read()
                .map_err(|e| FullTextSearchError::InternalError(format!("Failed to acquire index read lock: {}", e)))?;
            index
                .reader_builder()
                .reload_policy(ReloadPolicy::OnCommitWithDelay)
                .try_into()
                .map_err(|e| FullTextSearchError::InternalError(format!("Failed to create index reader: {}", e)))?
        };
        
        let searcher = index_reader.searcher();
        
        // Create query parser
        let query_parser = QueryParser::for_index(
            &searcher.index(),
            vec![
                self.schema.name_field,
                self.schema.description_field,
                self.schema.content_field,
                self.schema.keywords_field,
            ],
        );
        
        // Parse the query
        let parsed_query = query_parser
            .parse_query(&query.q)
            .map_err(|e| FullTextSearchError::QueryParsingError(format!("Failed to parse query: {}", e)))?;
        
        // Execute search
        let page = query.page.unwrap_or(1);
        let page_size = query.page_size.unwrap_or(20);
        let offset = (page - 1) * page_size;
        
        let top_docs = searcher
            .search(
                &parsed_query,
                &(TopDocs::with_limit(page_size).and_offset(offset)),
            )
            .map_err(|e| FullTextSearchError::SearchError(format!("Search execution failed: {}", e)))?;
        
        // Convert results
        let mut scored_artifacts = Vec::new();
        let mut max_score = 0.0f32;
        
        for (score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = match searcher.doc(doc_address) {
                Ok(doc) => doc,
                Err(e) => {
                    error!("Failed to retrieve document: {}", e);
                    continue;
                }
            };
            
            // Convert Tantivy document to our artifact representation
            if let Some(artifact_document) = self.schema.from_document(&retrieved_doc) {
                let highlight = Highlight {
                    field: "content".to_string(),
                    snippets: vec![], // In a real implementation, we would extract snippets
                };
                
                let scored_artifact = ScoredArtifact {
                    artifact: artifact_document,
                    score,
                    highlights: vec![highlight],
                };
                
                if score > max_score {
                    max_score = score;
                }
                
                scored_artifacts.push(scored_artifact);
            }
        }
        
        let query_time_ms = start_time.elapsed().as_millis();
        let total_count = scored_artifacts.len();
        
        Ok(FullTextSearchResults::new(
            scored_artifacts,
            total_count,
            page,
            page_size,
            query_time_ms,
            max_score,
        ))
    }
    
    async fn get_suggestions(
        &self,
        partial_query: &str,
        limit: usize,
    ) -> Result<Vec<String>, FullTextSearchError> {
        debug!(partial_query = %partial_query, limit = limit, "Getting search suggestions");
        
        // In a real implementation, we would query the index for suggestions
        // This is a placeholder implementation
        Ok(vec![])
    }
    
    async fn get_stats(&self) -> Result<SearchStats, FullTextSearchError> {
        debug!("Getting search engine stats");
        
        // In a real implementation, we would query the index for statistics
        // This is a placeholder implementation
        Ok(SearchStats {
            total_documents: 0,
            total_terms: 0,
            average_document_length: 0.0,
            index_size_bytes: 0,
            last_indexed_at: None,
        })
    }
}

#[async_trait]
impl IndexerPort for TantivySearchEngineAdapter {
    async fn index_artifact(
        &self,
        artifact: &IndexedArtifact,
    ) -> Result<(), FullTextSearchError> {
        debug!(artifact_id = %artifact.id, "Indexing artifact in Tantivy");
        
        let doc = self.schema.to_document(artifact);
        
        {
            let mut writer = self.index_writer.write()
                .map_err(|e| FullTextSearchError::IndexingError(format!("Failed to acquire index writer lock: {}", e)))?;
            writer
                .add_document(doc)
                .map_err(|e| FullTextSearchError::IndexingError(format!("Failed to add document to index: {}", e)))?;
            
            writer
                .commit()
                .map_err(|e| FullTextSearchError::IndexingError(format!("Failed to commit index changes: {}", e)))?;
        }
        
        info!(artifact_id = %artifact.id, "Artifact indexed successfully");
        Ok(())
    }
    
    async fn index_artifacts_batch(
        &self,
        artifacts: &[IndexedArtifact],
    ) -> Result<BatchIndexingResult, FullTextSearchError> {
        debug!(artifact_count = artifacts.len(), "Indexing artifacts batch in Tantivy");
        
        let start_time = std::time::Instant::now();
        let mut failed_count = 0;
        let mut errors = Vec::new();
        
        {
            let mut writer = self.index_writer.write()
                .map_err(|e| FullTextSearchError::BatchIndexingError(format!("Failed to acquire index writer lock: {}", e)))?;
            
            for artifact in artifacts {
                let doc = self.schema.to_document(artifact);
                if let Err(e) = writer.add_document(doc) {
                    failed_count += 1;
                    errors.push(format!("Failed to index artifact {}: {}", artifact.id, e));
                }
            }
            
            writer
                .commit()
                .map_err(|e| FullTextSearchError::BatchIndexingError(format!("Failed to commit batch index changes: {}", e)))?;
        }
        
        let duration_ms = start_time.elapsed().as_millis();
        
        info!(
            indexed_count = artifacts.len() - failed_count,
            failed_count = failed_count,
            duration_ms = duration_ms,
            "Batch indexing completed"
        );
        
        Ok(BatchIndexingResult {
            indexed_count: artifacts.len() - failed_count,
            failed_count,
            errors,
            duration_ms,
        })
    }
    
    async fn delete_artifact(
        &self,
        artifact_id: &str,
    ) -> Result<(), FullTextSearchError> {
        debug!(artifact_id = %artifact_id, "Deleting artifact from Tantivy index");
        
        // In a real implementation, we would delete the artifact from the index
        // This is a placeholder implementation
        Ok(())
    }
    
    async fn reindex_all(&self) -> Result<ReindexingResult, FullTextSearchError> {
        debug!("Reindexing all artifacts in Tantivy");
        
        // In a real implementation, we would reindex all artifacts
        // This is a placeholder implementation
        Ok(ReindexingResult {
            total_processed: 0,
            successful: 0,
            failed: 0,
            duration_ms: 0,
        })
    }
}

impl TokenizerPort for TantivySearchEngineAdapter {
    fn tokenize(&self, text: &str) -> Result<Vec<Token>, FullTextSearchError> {
        debug!(text_length = text.len(), "Tokenizing text");
        
        // In a real implementation, we would use Tantivy's tokenization capabilities
        // This is a simplified implementation
        let tokens: Vec<Token> = text
            .split_whitespace()
            .enumerate()
            .map(|(i, word)| Token {
                text: word.to_string(),
                position: i,
                start_offset: 0, // Would need to calculate actual offsets
                end_offset: 0,   // Would need to calculate actual offsets
            })
            .collect();
        
        Ok(tokens)
    }
    
    fn detect_language(&self, text: &str) -> Result<String, FullTextSearchError> {
        debug!(text_length = text.len(), "Detecting language");
        
        // In a real implementation, we would use a language detection library
        // This is a placeholder implementation
        Ok("en".to_string())
    }
    
    fn stem_tokens(&self, tokens: &[Token], language: &str) -> Result<Vec<Token>, FullTextSearchError> {
        debug!(token_count = tokens.len(), language = %language, "Stemming tokens");
        
        // In a real implementation, we would use a stemming library
        // This is a placeholder implementation that just returns the original tokens
        Ok(tokens.to_vec())
    }
}

impl ScorerPort for TantivySearchEngineAdapter {
    fn calculate_score(
        &self,
        query_terms: &[String],
        document_terms: &[String],
        document_length: usize,
    ) -> Result<f32, FullTextSearchError> {
        debug!(
            query_term_count = query_terms.len(),
            document_term_count = document_terms.len(),
            document_length = document_length,
            "Calculating relevance score"
        );
        
        // In a real implementation, we would use Tantivy's BM25 scoring
        // This is a simplified placeholder implementation
        let mut score = 0.0f32;
        let avg_doc_length = 100.0f32; // Placeholder average document length
        let k1 = 1.2f32; // BM25 parameter
        let b = 0.75f32; // BM25 parameter
        
        for query_term in query_terms {
            let term_freq = document_terms.iter().filter(|&t| t == query_term).count() as f32;
            if term_freq > 0.0 {
                let idf = ((1000.0 - term_freq + 0.5) / (term_freq + 0.5) + 1.0).ln(); // Simplified IDF
                let numerator = term_freq * (k1 + 1.0);
                let denominator = term_freq + k1 * (1.0 - b + b * (document_length as f32 / avg_doc_length));
                score += idf * (numerator / denominator);
            }
        }
        
        Ok(score)
    }
    
    fn normalize_scores(&self, scores: &[f32]) -> Result<Vec<f32>, FullTextSearchError> {
        debug!(score_count = scores.len(), "Normalizing scores");
        
        if scores.is_empty() {
            return Ok(vec![]);
        }
        
        let max_score = scores.iter().fold(0.0f32, |acc, &x| if x > acc { x } else { acc });
        
        if max_score == 0.0 {
            return Ok(scores.to_vec());
        }
        
        let normalized_scores: Vec<f32> = scores.iter().map(|&score| score / max_score).collect();
        Ok(normalized_scores)
    }
}

/// Search schema for Tantivy
#[derive(Debug, Clone)]
pub struct SearchSchema {
    pub schema: Schema,
    pub id_field: Field,
    pub name_field: Field,
    pub version_field: Field,
    pub description_field: Field,
    pub content_field: Field,
    pub package_type_field: Field,
    pub repository_field: Field,
    pub keywords_field: Field,
    pub authors_field: Field,
    pub licenses_field: Field,
    pub language_field: Field,
    pub indexed_at_field: Field,
}

impl SearchSchema {
    pub fn new() -> Self {
        let mut schema_builder = Schema::builder();
        
        // Create fields with appropriate types and options
        let id_field = schema_builder.add_text_field("id", STRING | STORED);
        let name_field = schema_builder.add_text_field("name", TEXT | STORED);
        let version_field = schema_builder.add_text_field("version", STRING | STORED);
        let description_field = schema_builder.add_text_field("description", TEXT | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT | STORED);
        let package_type_field = schema_builder.add_text_field("package_type", STRING | STORED);
        let repository_field = schema_builder.add_text_field("repository", STRING | STORED);
        let keywords_field = schema_builder.add_text_field("keywords", TEXT | STORED);
        let authors_field = schema_builder.add_text_field("authors", TEXT | STORED);
        let licenses_field = schema_builder.add_text_field("licenses", TEXT | STORED);
        let language_field = schema_builder.add_text_field("language", STRING | STORED);
        let indexed_at_field = schema_builder.add_date_field("indexed_at", INDEXED | STORED);
        
        let schema = schema_builder.build();
        
        Self {
            schema,
            id_field,
            name_field,
            version_field,
            description_field,
            content_field,
            package_type_field,
            repository_field,
            keywords_field,
            authors_field,
            licenses_field,
            language_field,
            indexed_at_field,
        }
    }
    
    pub fn to_document(&self, artifact: &IndexedArtifact) -> TantivyDocument {
        tantivy::doc! {
            self.id_field => artifact.id.clone(),
            self.name_field => artifact.metadata.name.clone(),
            self.version_field => artifact.metadata.version.clone(),
            self.description_field => artifact.metadata.description.clone(),
            self.content_field => artifact.content.clone(),
            self.package_type_field => artifact.metadata.package_type.clone(),
            self.repository_field => artifact.metadata.repository.clone(),
            self.keywords_field => artifact.metadata.keywords.join(" "),
            self.authors_field => artifact.metadata.authors.join(" "),
            self.licenses_field => artifact.metadata.licenses.join(" "),
            self.language_field => artifact.language.clone(),
            self.indexed_at_field => tantivy::DateTime::from_utc(artifact.indexed_at),
        }
    }
    
    pub fn from_document(&self, doc: &TantivyDocument) -> Option<crate::features::basic_search::dto::ArtifactDocument> {
        let id = doc.get_first(self.id_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let name = doc.get_first(self.name_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let version = doc.get_first(self.version_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let package_type = doc.get_first(self.package_type_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        let repository = doc.get_first(self.repository_field)
            .and_then(|v| v.as_text())
            .map(|s| s.to_string())?;
            
        Some(crate::features::basic_search::dto::ArtifactDocument {
            id,
            name,
            version,
            package_type,
            repository,
        })
    }
}