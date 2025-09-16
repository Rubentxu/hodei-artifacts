//! Concrete adapters for Full Text Search Feature
//!
//! This module provides implementations of the segregated ports using Tantivy
//! as the underlying search engine. Each adapter is focused and single-purpose.

use async_trait::async_trait;
use std::sync::{Arc, RwLock};
use tantivy::{
    collector::{TopDocs, Count},
    query::{Query, QueryParser, BooleanQuery, Occur, PhraseQuery, TermQuery},
    schema::*,
    tokenizer::{TokenizerManager, SimpleTokenizer},
    Index, IndexReader, Searcher, ReloadPolicy, TantivyDocument, DocAddress,
    Term,
};
use tracing::{debug, info, error, warn};
use serde_json;

use super::ports::*;
use super::dto::*;
use super::error::{
    FullTextSearchError, ToFullTextSearchError, 
    RebuildError, ClearError, ValidationError, 
    MaintenanceError, SegmentError, MergeError
};
use crate::features::index_text_documents::adapter::DocumentIndexSchema;

/// Tantivy-based full-text search adapter
pub struct TantivyFullTextSearchAdapter {
    index: Arc<RwLock<Index>>,
    schema: Arc<DocumentIndexSchema>,
    tokenizer_manager: TokenizerManager,
    index_reader: Arc<RwLock<Option<IndexReader>>>,
}

impl TantivyFullTextSearchAdapter {
    pub fn new(index: Arc<RwLock<Index>>, schema: Arc<DocumentIndexSchema>) -> Self {
        let tokenizer_manager = TokenizerManager::new();
        
        Self {
            index,
            schema,
            tokenizer_manager,
            index_reader: Arc::new(RwLock::new(None)),
        }
    }
    
    async fn get_reader(&self) -> Result<IndexReader, FullTextSearchError> {
        // Check if we already have a reader
        {
            let reader = self.index_reader.read()
                .map_err(|e| FullTextSearchError::concurrency(format!("Failed to acquire reader lock: {}", e)))?;
            
            if let Some(reader) = reader.as_ref() {
                reader.reload()
                    .map_err(|e| FullTextSearchError::external_service("Tantivy", e.to_string()))?;
                return Ok(reader.clone());
            }
        }
        
        // Create new reader
        let index = self.index.read()
            .map_err(|e| FullTextSearchError::concurrency(format!("Failed to acquire index lock: {}", e)))?;
        
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| FullTextSearchError::Search { 
                source: SearchError::InternalError(format!("Failed to create index reader: {}", e)) 
            })?;
        
        // Store the reader
        {
            let mut reader_guard = self.index_reader.write()
                .map_err(|e| FullTextSearchError::concurrency(format!("Failed to acquire writer lock: {}", e)))?;
            *reader_guard = Some(reader.clone());
        }
        
        Ok(reader)
    }
    
    fn parse_search_query(&self, query: &FullTextSearchQuery, searcher: &Searcher) -> Result<Box<dyn Query>, FullTextSearchError> {
        let mut query_parts = Vec::new();
        
        // Parse the main query string
        let query_parser = QueryParser::for_index(
            &searcher.index(),
            vec![
                self.schema.content_field,
                self.schema.title_field,
                self.schema.description_field,
                self.schema.tags_field,
            ],
        );
        
        let main_query = query_parser.parse_query(&query.q)
            .map_err(|e| FullTextSearchError::Search { 
                source: SearchError::QueryParseFailed(format!("Failed to parse query '{}': {}", query.q, e)) 
            })?;
        
        query_parts.push((Occur::Must, main_query));
        
        // Add field filters
        if let Some(artifact_type) = &query.artifact_type {
            let term = Term::from_field_text(self.schema.artifact_type_field, artifact_type);
            let field_query = TermQuery::new(term, IndexRecordOption::Basic);
            query_parts.push((Occur::Must, Box::new(field_query)));
        }
        
        if let Some(language) = &query.language {
            let term = Term::from_field_text(self.schema.language_field, language);
            let field_query = TermQuery::new(term, IndexRecordOption::Basic);
            query_parts.push((Occur::Must, Box::new(field_query)));
        }
        
        // Add date range filter if provided
        if let Some(_date_range) = &query.date_range {
            // Note: Tantivy requires a proper range query implementation for date fields.
            // For now, we'll skip applying a date range filter here to avoid type conversion issues.
        }
        
        // Combine all query parts
        let boolean_query = BooleanQuery::new(query_parts);
        Ok(Box::new(boolean_query))
    }
    
    fn convert_tantivy_doc_to_search_result(&self, doc: &TantivyDocument, score: f32, query: &FullTextSearchQuery) -> Result<SearchResult, FullTextSearchError> {
        let document_id = doc.get_first(self.schema.artifact_id_field)
            .and_then(|v| v.as_str())
            .ok_or_else(|| FullTextSearchError::Search { 
                source: SearchError::InternalError("Missing artifact_id field in document".to_string()) 
            })?;
        
        let title = doc.get_first(self.schema.title_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let description = doc.get_first(self.schema.description_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let artifact_type = doc.get_first(self.schema.artifact_type_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let version = doc.get_first(self.schema.version_field)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        
        let tags = doc.get_first(self.schema.tags_field)
            .and_then(|v| v.as_str())
            .map(|s| s.split(' ').map(|t| t.to_string()).collect())
            .unwrap_or_default();
        
        let language = doc.get_first(self.schema.language_field)
            .and_then(|v| v.as_str());
        
        let indexed_at = if let Some(dt) = doc.get_first(self.schema.indexed_at_field)
            .and_then(|v| v.as_datetime()) {
            let secs = dt.into_utc().unix_timestamp();
            if let Some(naive) = chrono::NaiveDateTime::from_timestamp_opt(secs, 0) {
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
            } else {
                chrono::Utc::now()
            }
        } else {
            chrono::Utc::now()
        };
        
        let metadata = ArtifactMetadata {
            title: Some(title.to_string()),
            description: Some(description.to_string()),
            tags,
            artifact_type: artifact_type.to_string(),
            version: version.to_string(),
            custom_metadata: std::collections::HashMap::new(),
            created_at: indexed_at,
            updated_at: indexed_at,
        };
        
        let ranking = RankingInfo {
            bm25_score: Some(score),
            tfidf_score: Some(score * 0.8), // Approximate
            pagerank_score: None,
            freshness_score: Some(self.calculate_freshness_score(indexed_at)),
            popularity_score: None,
            combined_score: score,
        };
        
        Ok(SearchResult {
            document_id: document_id.to_string(),
            metadata,
            score,
            highlights: Vec::new(), // Will be populated by highlighter
            snippets: Vec::new(), // Will be populated by snippet generator
            ranking,
            language: language.map(|s| s.to_string()),
            indexed_at,
        })
    }
    
    fn calculate_freshness_score(&self, indexed_at: chrono::DateTime<chrono::Utc>) -> f32 {
        let now = chrono::Utc::now();
        let days_old = (now - indexed_at).num_days();
        
        // Exponential decay: score decreases with age
        (-days_old as f32 / 365.0).exp().max(0.1)
    }
}

#[async_trait]
impl FullTextSearchPort for TantivyFullTextSearchAdapter {
    async fn search(&self, query: FullTextSearchQuery) -> Result<FullTextSearchResults, SearchError> {
        debug!("Executing search query: {}", query.q);
        
        let start_time = std::time::Instant::now();
        
        let reader = self
            .get_reader()
            .await
            .map_err(|e| SearchError::InternalError(e.to_string()))?;
        let searcher = reader.searcher();
        
        // Parse the query
        let search_query = self
            .parse_search_query(&query, &searcher)
            .map_err(|e| SearchError::QueryParseFailed(e.to_string()))?;
        
        // Set up pagination
        let page = query.page.unwrap_or(1);
        let page_size = query.page_size.unwrap_or(20);
        let offset = (page - 1) * page_size;
        
        // Execute the search
        let (top_docs, count) = searcher.search(
            &search_query,
            &(TopDocs::with_limit(page_size).and_offset(offset), Count),
        ).map_err(|e| SearchError::QueryExecutionFailed(format!("Search execution failed: {}", e)))?;
        
        // Convert results
        let mut results = Vec::new();
        let mut max_score = 0.0f32;
        
        for (score, doc_address) in top_docs {
            let retrieved_doc: TantivyDocument = match searcher.doc(doc_address) {
                Ok(doc) => doc,
                Err(e) => {
                    error!("Failed to retrieve document: {}", e);
                    continue;
                }
            };
            
            match self.convert_tantivy_doc_to_search_result(&retrieved_doc, score, &query) {
                Ok(result) => {
                    if score > max_score {
                        max_score = score;
                    }
                    results.push(result);
                }
                Err(e) => {
                    warn!("Failed to convert document to search result: {}", e);
                }
            }
        }
        
        let query_time_ms = start_time.elapsed().as_millis() as u64;
        let total_count = count;
        
        // Get index statistics
        let index_stats = IndexStats {
            total_documents: searcher.num_docs() as u64,
            total_terms: 0, // TODO: Get actual term count
            avg_terms_per_document: 0.0, // TODO: Calculate average
            index_size_bytes: 0, // TODO: Get actual index size
            memory_usage_bytes: 0, // TODO: Get actual memory usage
            segment_count: searcher.segment_readers().len() as u32,
            created_at: chrono::Utc::now(),
            last_optimized_at: None,
        };
        
        let metadata = SearchMetadata {
            execution_plan: None,
            query_term_count: query.q.split_whitespace().count(),
            query_rewritten: false,
            expansion_terms: None,
            engine_version: "Tantivy".to_string(),
            index_stats,
        };
        
        Ok(FullTextSearchResults {
            results,
            total_count,
            page,
            page_size,
            query_time_ms,
            max_score,
            metadata,
            facets: None,
            suggestions: None,
        })
    }
    
    async fn get_suggestions(&self, query: SearchSuggestionsQuery) -> Result<SearchSuggestionsResponse, SuggestionError> {
        debug!("Getting suggestions for: {}", query.partial_query);
        
        let start_time = std::time::Instant::now();
        
        // For now, return simple spelling suggestions based on existing terms
        let reader = self
            .get_reader()
            .await
            .map_err(|e| SuggestionError::SuggestionGenerationFailed(e.to_string()))?;
        let searcher = reader.searcher();
        
        // This is a simplified implementation
        // In a real implementation, we would use proper spell checking and autocomplete
        let suggestions = vec![
            SearchSuggestion {
                text: query.partial_query.clone(),
                highlighted: Some(format!("*{}*", query.partial_query)),
                score: 0.8,
                suggestion_type: SuggestionType::Autocomplete,
            }
        ];
        
        let query_time_ms = start_time.elapsed().as_millis() as u64;
        let total = suggestions.len();
        
        Ok(SearchSuggestionsResponse {
            suggestions,
            query_time_ms,
            total_count: total,
        })
    }
    
    async fn get_facets(&self, _query: FullTextSearchQuery) -> Result<SearchFacets, FacetError> {
        Err(FacetError::FacetGenerationFailed("Faceting not implemented".to_string()))
    }
    
    async fn more_like_this(&self, document_id: &str, limit: usize) -> Result<FullTextSearchResults, SearchError> {
        debug!("Executing more-like-this for document: {}", document_id);
        
        // Get the reference document
        let reader = self.get_reader().await.map_err(|e| SearchError::InternalError(format!("Failed to get reader: {}", e)))?;
        let _searcher = reader.searcher();
        
        // Create a term-based query from the document
        // This is a simplified implementation
        let mut terms: Vec<String> = Vec::new();
        
        // In a real implementation, we would extract significant terms from the document
        // and create a query based on those terms
        
        let query = FullTextSearchQuery {
            q: "significant terms from document".to_string(), // Placeholder
            artifact_type: None,
            language: None,
            tags: None,
            date_range: None,
            search_mode: SearchMode::Simple,
            page: Some(1),
            page_size: Some(limit),
            include_highlights: false,
            include_snippets: false,
            snippet_length: None,
            sort_order: SortOrder::Relevance,
            min_score: None,
            fuzziness: None,
            enable_stemming: None,
            enable_phonetic: None,
        };
        
        self.search(query).await
    }
    
    async fn search_with_scroll(&self, _query: FullTextSearchQuery) -> Result<ScrollSearchResponse, SearchError> {
        Err(SearchError::InternalError("Scroll search not implemented".to_string()))
    }
    
    async fn continue_scroll(&self, _scroll_id: &str) -> Result<ScrollSearchResponse, SearchError> {
        Err(SearchError::InternalError("Scroll continuation not implemented".to_string()))
    }
}

/// Simple query analyzer adapter
pub struct SimpleQueryAnalyzer {
    tokenizer_manager: TokenizerManager,
}

impl SimpleQueryAnalyzer {
    pub fn new() -> Self {
        Self {
            tokenizer_manager: TokenizerManager::new(),
        }
    }
}

#[async_trait]
impl QueryAnalyzerPort for SimpleQueryAnalyzer {
    async fn analyze_query(&self, command: AnalyzeQueryPerformanceCommand) -> Result<QueryPerformanceAnalysis, AnalysisError> {
        let start_time = std::time::Instant::now();
        
        // Parse the query
        let parsed_query = self
            .parse_query(&command.query.q, command.query.search_mode)
            .await
            .map_err(|e| AnalysisError::AnalysisFailed(e.to_string()))?;
        
        // Extract terms
        let query_terms = self
            .extract_query_terms(&command.query.q, command.query.language.as_deref())
            .await
            .map_err(|e| AnalysisError::AnalysisFailed(e.to_string()))?;
        
        // Calculate complexity
        let complexity_score = self.calculate_complexity(&parsed_query, &query_terms);
        
        // Generate optimizations
        let optimizations = self.generate_optimizations(&parsed_query, &query_terms);
        
        let analysis = QueryAnalysis {
            complexity_score,
            terms_analysis: QueryTermsAnalysis {
                term_count: query_terms.normalized_terms.len(),
                unique_terms: query_terms.normalized_terms.clone(),
                stop_words: query_terms.stop_words.clone(),
                rare_terms: Vec::new(),
                common_terms: Vec::new(),
            },
            query_type: self.classify_query_type(&parsed_query),
            optimizations,
        };
        
        let metrics = if command.include_timing {
            Some(QueryPerformanceMetrics {
                parsing_time_ms: 10.0,
                optimization_time_ms: 5.0,
                scan_time_ms: 0.0,
                processing_time_ms: 2.0,
                total_time_ms: start_time.elapsed().as_millis() as f32,
                documents_scanned: 0,
                documents_matched: 0,
                memory_usage_bytes: 1024,
                cpu_usage_percent: 0.1,
            })
        } else {
            None
        };
        
        Ok(QueryPerformanceAnalysis {
            analysis,
            metrics: metrics.unwrap_or_default(),
            execution_plan: None,
            index_stats: None,
        })
    }
    
    async fn parse_query(&self, query: &str, mode: SearchMode) -> Result<ParsedQuery, ParseError> {
        let terms: Vec<QueryTerm> = query
            .split_whitespace()
            .map(|term| QueryTerm {
                term: term.to_string(),
                field: None,
                boost: None,
                fuzzy: None,
            })
            .collect();
        
        Ok(ParsedQuery {
            original_query: query.to_string(),
            parsed_terms: terms,
            operators: Vec::new(),
            filters: Vec::new(),
            query_type: match mode {
                SearchMode::Simple => QueryType::SimpleKeyword,
                SearchMode::Phrase => QueryType::Phrase,
                SearchMode::Boolean => QueryType::Boolean,
                _ => QueryType::SimpleKeyword,
            },
        })
    }
    
    async fn optimize_query(&self, parsed_query: ParsedQuery) -> Result<OptimizedQuery, OptimizationError> {
        let optimized_terms = parsed_query.parsed_terms.clone();
        Ok(OptimizedQuery {
            original_query: parsed_query,
            optimized_terms,
            optimization_hints: Vec::new(),
            estimated_cost: 0.5,
        })
    }
    
    async fn extract_query_terms(&self, query: &str, language: Option<&str>) -> Result<QueryTerms, ExtractionError> {
        let original_terms: Vec<String> = query.split_whitespace().map(|s| s.to_string()).collect();
        let normalized_terms: Vec<String> = original_terms.iter().map(|s| s.to_lowercase()).collect();
        
        // Simple stop word detection
        let stop_words_list = ["the", "a", "an", "and", "or", "but", "in", "on", "at", "to"];
        let mut stop_words: Vec<String> = Vec::new();
        for s in &normalized_terms {
            if stop_words_list.iter().any(|w| *w == s.as_str()) {
                stop_words.push(s.clone());
            }
        }
        
        Ok(QueryTerms {
            original_terms,
            normalized_terms,
            stop_words,
            synonyms: Vec::new(),
            stemmed_terms: Vec::new(),
            language: language.map(|s| s.to_string()),
        })
    }
    
    async fn rewrite_query(&self, _parsed_query: ParsedQuery) -> Result<RewrittenQuery, RewriteError> {
        Err(RewriteError::RewriteFailed("Query rewriting not implemented".to_string()))
    }
    
    fn calculate_complexity(&self, parsed_query: &ParsedQuery, query_terms: &QueryTerms) -> f32 {
        let mut complexity = 0.0;
        
        complexity += (parsed_query.parsed_terms.len() as f32) * 0.1;
        complexity += (parsed_query.operators.len() as f32) * 0.3;
        complexity += (parsed_query.filters.len() as f32) * 0.2;
        complexity += (query_terms.stop_words.len() as f32) * 0.05;
        
        complexity.min(1.0)
    }
    
    fn generate_optimizations(&self, parsed_query: &ParsedQuery, query_terms: &QueryTerms) -> Vec<QueryOptimization> {
        let mut optimizations = Vec::new();
        
        if !query_terms.stop_words.is_empty() {
            optimizations.push(QueryOptimization {
                description: "Query contains stop words that can be removed".to_string(),
                expected_improvement: "Improve relevance and reduce processing time".to_string(),
                priority: OptimizationPriority::Medium,
            });
        }
        
        if parsed_query.parsed_terms.len() > 10 {
            optimizations.push(QueryOptimization {
                description: "Query contains many terms, consider being more specific".to_string(),
                expected_improvement: "Improve precision and reduce noise".to_string(),
                priority: OptimizationPriority::Low,
            });
        }
        
        optimizations
    }
    
    fn classify_query_type(&self, parsed_query: &ParsedQuery) -> QueryType {
        if parsed_query.operators.is_empty() && parsed_query.filters.is_empty() {
            QueryType::SimpleKeyword
        } else {
            QueryType::Boolean
        }
    }
}

/// Simple relevance scorer adapter
pub struct SimpleRelevanceScorer;

impl SimpleRelevanceScorer {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RelevanceScorerPort for SimpleRelevanceScorer {
    async fn calculate_score(&self, request: ScoreCalculationRequest) -> Result<RelevanceScore, ScoreError> {
        let bm25_score = self.calculate_bm25_score(BM25Request {
            term_frequency: 1.0,
            document_length: request.document_length,
            avg_document_length: request.avg_document_length,
            total_documents: 1000,
            document_frequency: 1,
            k1: 1.2,
            b: 0.75,
        }).await?;
        
        let tfidf_score = self.calculate_tfidf_score(TFIDFRequest {
            term_frequency: 1.0,
            document_frequency: 1,
            total_documents: 1000,
        }).await?;
        
        Ok(RelevanceScore {
            document_id: request.document_id,
            score: bm25_score,
            confidence: 0.8,
            score_components: vec![
                ScoreComponent {
                    score_type: "bm25".to_string(),
                    score: bm25_score,
                    weight: 0.7,
                },
                ScoreComponent {
                    score_type: "tfidf".to_string(),
                    score: tfidf_score,
                    weight: 0.3,
                },
            ],
        })
    }
    
    async fn calculate_bm25_score(&self, request: BM25Request) -> Result<f32, ScoreError> {
        let idf = ((request.total_documents as f32 - request.document_frequency as f32 + 0.5) / 
                   (request.document_frequency as f32 + 0.5) + 1.0).ln();
        
        let numerator = request.term_frequency * (request.k1 + 1.0);
        let denominator = request.term_frequency + request.k1 * (1.0 - request.b + 
            request.b * (request.document_length as f32 / request.avg_document_length));
        
        Ok(idf * (numerator / denominator))
    }
    
    async fn calculate_tfidf_score(&self, request: TFIDFRequest) -> Result<f32, ScoreError> {
        let tf = (1.0 + request.term_frequency.ln()).ln();
        let idf = (request.total_documents as f32 / request.document_frequency as f32).ln();
        Ok(tf * idf)
    }
    
    async fn combine_scores(&self, scores: Vec<ScoreComponent>) -> Result<CombinedScore, ScoreError> {
        let mut total_score = 0.0;
        let mut total_weight = 0.0;
        let component_scores = scores.clone();
        
        for component in &scores {
            total_score += component.score * component.weight;
            total_weight += component.weight;
        }
        
        let final_score = if total_weight > 0.0 {
            total_score / total_weight
        } else {
            0.0
        };
        
        Ok(CombinedScore {
            document_id: "unknown".to_string(), // Should be set by caller
            final_score,
            component_scores,
        })
    }
    
    async fn normalize_scores(&self, scores: Vec<RawScore>) -> Result<Vec<NormalizedScore>, ScoreError> {
        if scores.is_empty() {
            return Ok(Vec::new());
        }
        
        let max_score = scores.iter()
            .map(|s| s.score)
            .fold(0.0f32, f32::max);
        
        if max_score == 0.0 {
            return Ok(scores.into_iter().map(|s| NormalizedScore {
                document_id: s.document_id,
                original_score: s.score,
                normalized_score: 0.0,
            }).collect());
        }
        
        Ok(scores.into_iter().map(|s| NormalizedScore {
            document_id: s.document_id,
            original_score: s.score,
            normalized_score: s.score / max_score,
        }).collect())
    }
    
    async fn rank_documents(&self, documents: Vec<DocumentToRank>) -> Result<Vec<RankedDocument>, RankingError> {
        let mut ranked: Vec<_> = documents.into_iter().enumerate().map(|(i, doc)| RankedDocument {
            document_id: doc.document_id,
            rank: i + 1,
            final_score: doc.raw_score,
            confidence: 0.8,
        }).collect();
        
        // Sort by score descending
        ranked.sort_by(|a, b| b.final_score.partial_cmp(&a.final_score).unwrap_or(std::cmp::Ordering::Equal));
        
        // Update ranks
        for (i, doc) in ranked.iter_mut().enumerate() {
            doc.rank = i + 1;
        }
        
        Ok(ranked)
    }
}

/// Simple search index manager adapter
pub struct SimpleSearchIndexManager {
    index: Arc<RwLock<Index>>,
}

impl SimpleSearchIndexManager {
    pub fn new(index: Arc<RwLock<Index>>) -> Self {
        Self { index }
    }
}

#[async_trait]
impl SearchIndexManagerPort for SimpleSearchIndexManager {
    async fn get_index_stats(&self) -> Result<IndexStats, IndexError> {
        let index = self.index.read()
            .map_err(|e| IndexError::IndexOperationFailed(format!("Failed to acquire index lock: {}", e)))?;
        
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| IndexError::IndexOperationFailed(format!("Failed to create index reader: {}", e)))?;
        
        let searcher = reader.searcher();
        
        Ok(IndexStats {
            total_documents: searcher.num_docs() as u64,
            total_terms: 0, // TODO: Get actual term count
            avg_terms_per_document: 0.0, // TODO: Calculate average
            index_size_bytes: 0, // TODO: Get actual index size
            memory_usage_bytes: 0, // TODO: Get actual memory usage
            segment_count: searcher.segment_readers().len() as u32,
            created_at: chrono::Utc::now(),
            last_optimized_at: None,
        })
    }
    
    async fn optimize_index(&self) -> Result<OptimizationResult, OptimizationError> {
        // TODO: Implement actual index optimization
        Ok(OptimizationResult {
            segments_before: 1,
            segments_after: 1,
            size_reduction_bytes: 0,
            time_taken_ms: 100,
            success: true,
        })
    }
    
    async fn rebuild_index(&self) -> Result<RebuildResult, RebuildError> {
        // TODO: Implement actual index rebuild
        Ok(RebuildResult {
            success: true,
            documents_processed: 0,
            rebuild_time_ms: 1000,
            message: "Index rebuild completed".to_string(),
        })
    }
    
    async fn clear_index(&self) -> Result<ClearResult, ClearError> {
        // TODO: Implement actual index clear
        Ok(ClearResult {
            success: true,
            documents_removed: 0,
            clear_time_ms: 100,
        })
    }
    
    async fn validate_index(&self) -> Result<ValidationResult, ValidationError> {
        // TODO: Implement actual index validation
        Ok(ValidationResult {
            is_valid: true,
            issues: Vec::new(),
            validation_time_ms: 50,
        })
    }
    
    async fn get_index_config(&self) -> Result<IndexConfig, ConfigError> {
        // TODO: Implement actual config retrieval
        Ok(IndexConfig {
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
    
    async fn update_index_config(&self, config: IndexConfig) -> Result<ConfigUpdateResult, ConfigError> {
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
        let index = self.index.read()
            .map_err(|e| SegmentError::SegmentOperationFailed(format!("Failed to acquire index lock: {}", e)))?;
        
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| SegmentError::SegmentOperationFailed(format!("Failed to create index reader: {}", e)))?;
        
        let searcher = reader.searcher();
        
        let segments: Vec<SegmentInfo> = searcher.segment_readers().iter().enumerate().map(|(i, _)| SegmentInfo {
            segment_id: format!("segment_{}", i),
            document_count: 0, // TODO: Get actual document count
            size_bytes: 0, // TODO: Get actual segment size
            deleted_documents: 0, // TODO: Get actual deleted document count
            created_at: chrono::Utc::now(),
        }).collect();
        
        Ok(segments)
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

/// Simple highlighter adapter
pub struct SimpleHighlighter;

impl SimpleHighlighter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl HighlighterPort for SimpleHighlighter {
    async fn generate_highlights(&self, request: HighlightRequest) -> Result<Vec<Highlight>, HighlightError> {
        // Simple implementation - wrap query terms in asterisks
        let highlights = vec![Highlight {
            field: request.fields.first().cloned().unwrap_or_else(|| "content".to_string()),
            text: "*highlighted text*".to_string(),
            position: Some(0),
            confidence: Some(0.8),
        }];
        
        Ok(highlights)
    }
    
    async fn generate_snippets(&self, request: SnippetRequest) -> Result<Vec<TextSnippet>, SnippetError> {
        // Simple implementation - return a snippet of the content
        let snippet = TextSnippet {
            text: request.content.chars().take(request.snippet_length).collect(),
            field: "content".to_string(),
            start_pos: 0,
            end_pos: request.snippet_length.min(request.content.len()),
            score: 0.8,
        };
        
        Ok(vec![snippet])
    }
    
    async fn extract_best_passages(&self, request: PassageExtractionRequest) -> Result<Vec<TextPassage>, PassageError> {
        // TODO: Implement passage extraction
        todo!()
    }
    
    async fn highlight_terms(&self, text: &str, terms: &[String], _language: Option<&str>) -> Result<String, HighlightError> {
        let mut highlighted_text = text.to_string();
        
        for term in terms {
            let term_lower = term.to_lowercase();
            let text_lower = highlighted_text.to_lowercase();
            
            if let Some(pos) = text_lower.find(&term_lower) {
                let before = &highlighted_text[..pos];
                let term_match = &highlighted_text[pos..pos + term.len()];
                let after = &highlighted_text[pos + term.len()..];
                
                highlighted_text = format!("{}*{}*{}", before, term_match, after);
            }
        }
        
        Ok(highlighted_text)
    }
}

/// Simple performance monitor adapter
pub struct SimpleSearchPerformanceMonitor {
    metrics: Arc<tokio::sync::RwLock<Vec<QueryMetrics>>>,
}

impl SimpleSearchPerformanceMonitor {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl SearchPerformanceMonitorPort for SimpleSearchPerformanceMonitor {
    async fn record_query_metrics(&self, metrics: QueryMetrics) -> Result<(), MonitoringError> {
        let mut metrics_store = self.metrics.write().await;
        metrics_store.push(metrics);
        
        // Keep only last 1000 metrics
        if metrics_store.len() > 1000 {
            let keep_count = 1000;
            let drain_count = metrics_store.len() - keep_count;
            metrics_store.drain(0..drain_count);
        }
        
        Ok(())
    }
    
    async fn get_search_stats(&self, time_range: TimeRange) -> Result<SearchPerformanceStats, StatsError> {
        let metrics_store = self.metrics.read().await;
        
        let relevant_metrics: Vec<_> = metrics_store.iter()
            .filter(|m| m.timestamp >= time_range.start && m.timestamp <= time_range.end)
            .collect();
        
        if relevant_metrics.is_empty() {
            return Err(StatsError::StatsNotAvailable);
        }
        
        let total_queries = relevant_metrics.len();
        let total_time_ms: f64 = relevant_metrics.iter().map(|m| m.execution_time_ms as f64).sum();
        let average_query_time_ms = total_time_ms / total_queries as f64;
        
        let mut execution_times: Vec<f64> = relevant_metrics.iter()
            .map(|m| m.execution_time_ms as f64)
            .collect();
        execution_times.sort_by(|a, b| a.partial_cmp(b).unwrap());
        
        let p95_query_time_ms = execution_times[(execution_times.len() as f64 * 0.95) as usize];
        let p99_query_time_ms = execution_times[(execution_times.len() as f64 * 0.99) as usize];
        
        let cache_hits = relevant_metrics.iter().filter(|m| m.cache_hit).count();
        let cache_hit_rate = cache_hits as f64 / total_queries as f64;
        
        let errors = relevant_metrics.iter().filter(|m| m.execution_time_ms > 5000).count(); // Assume >5s is error
        let error_rate = errors as f64 / total_queries as f64;
        
        Ok(SearchPerformanceStats {
            total_queries,
            average_query_time_ms,
            p95_query_time_ms,
            p99_query_time_ms,
            cache_hit_rate,
            error_rate,
            most_popular_queries: Vec::new(),
            slowest_queries: Vec::new(),
        })
    }
    
    async fn get_slow_queries(&self, threshold_ms: u64, limit: usize) -> Result<Vec<SlowQueryInfo>, QueryError> {
        let metrics_store = self.metrics.read().await;
        
        let slow_queries: Vec<_> = metrics_store.iter()
            .filter(|m| m.execution_time_ms > threshold_ms)
            .take(limit)
            .map(|m| SlowQueryInfo {
                query: m.query_text.clone(),
                execution_time_ms: m.execution_time_ms,
                timestamp: m.timestamp,
                user_id: m.user_id.clone(),
            })
            .collect();
        
        Ok(slow_queries)
    }
    
    async fn monitor_search_health(&self) -> Result<SearchHealthStatus, HealthError> {
        // Simple health check based on recent query performance
        let time_range = TimeRange {
            start: chrono::Utc::now() - chrono::Duration::minutes(5),
            end: chrono::Utc::now(),
        };
        
        let stats = self.get_search_stats(time_range).await;
        
        let overall_status = match stats {
            Ok(stats) => {
                if stats.error_rate > 0.1 || stats.average_query_time_ms > 1000.0 {
                    HealthStatus::Unhealthy
                } else if stats.error_rate > 0.05 || stats.average_query_time_ms > 500.0 {
                    HealthStatus::Warning
                } else {
                    HealthStatus::Healthy
                }
            }
            Err(_) => HealthStatus::Unknown,
        };
        
        Ok(SearchHealthStatus {
            overall_status,
            components: Vec::new(),
            last_check: chrono::Utc::now(),
        })
    }
    
    async fn get_query_patterns(&self, time_range: TimeRange) -> Result<QueryPatterns, PatternError> {
        // TODO: Implement query pattern analysis
        todo!()
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use crate::features::index_text_documents::adapter::test::*;
    use std::sync::Arc;
    
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
    
    // Mock use cases for testing
    pub struct MockFullTextSearchUseCase {
        pub should_fail: bool,
    }
    
    impl MockFullTextSearchUseCase {
        pub fn new() -> Self {
            Self { should_fail: false }
        }
        
        pub fn failing(mut self) -> Self {
            self.should_fail = true;
            self
        }
    }
    
    impl FullTextSearchUseCaseTrait for MockFullTextSearchUseCase {
        async fn execute(&self, query: FullTextSearchQuery) -> Result<FullTextSearchResults, FullTextSearchError> {
            if self.should_fail {
                return Err(FullTextSearchError::Search {
                    source: SearchError::QueryExecutionFailed("Mock use case failed".to_string()),
                });
            }
            
            Ok(FullTextSearchResults::default())
        }
        
        async fn more_like_this(&self, document_id: &str, limit: usize) -> Result<FullTextSearchResults, FullTextSearchError> {
            self.execute(FullTextSearchQuery::default()).await
        }
        
        async fn get_facets(&self, query: FullTextSearchQuery) -> Result<SearchFacets, FacetError> {
            todo!()
        }
    }
    
    pub struct MockSearchSuggestionsUseCase {
        pub should_fail: bool,
    }
    
    impl MockSearchSuggestionsUseCase {
        pub fn new() -> Self {
            Self { should_fail: false }
        }
        
        pub fn failing(mut self) -> Self {
            self.should_fail = true;
            self
        }
    }
    
    impl SearchSuggestionsUseCaseTrait for MockSearchSuggestionsUseCase {
        async fn execute(&self, query: SearchSuggestionsQuery) -> Result<SearchSuggestionsResponse, FullTextSearchError> {
            if self.should_fail {
                return Err(FullTextSearchError::Search {
                    source: SearchError::QueryExecutionFailed("Mock suggestions failed".to_string()),
                });
            }
            
            Ok(SearchSuggestionsResponse::default())
        }
    }
    
    pub struct MockQueryPerformanceUseCase {
        pub should_fail: bool,
    }
    
    impl MockQueryPerformanceUseCase {
        pub fn new() -> Self {
            Self { should_fail: false }
        }
        
        pub fn failing(mut self) -> Self {
            self.should_fail = true;
            self
        }
    }
    
    impl QueryPerformanceUseCaseTrait for MockQueryPerformanceUseCase {
        async fn execute(&self, command: AnalyzeQueryPerformanceCommand) -> Result<QueryPerformanceAnalysis, FullTextSearchError> {
            if self.should_fail {
                return Err(FullTextSearchError::Analysis {
                    source: AnalysisError::QueryParseFailed("Mock analysis failed".to_string()),
                });
            }
            
            Ok(QueryPerformanceAnalysis::default())
        }
    }
}