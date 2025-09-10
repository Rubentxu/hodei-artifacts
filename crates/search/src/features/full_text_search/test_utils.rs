use async_trait::async_trait;
use std::sync::{Arc, RwLock};
use crate::features::full_text_search::{
    ports::{
        SearchEnginePort, IndexerPort, TokenizerPort, ScorerPort,
        SearchStats, BatchIndexingResult, ReindexingResult, Token,
    },
    dto::{FullTextSearchQuery, FullTextSearchResults, IndexedArtifact},
    error::FullTextSearchError,
};

/// Mock search engine adapter for testing
#[derive(Debug, Clone)]
pub struct MockSearchEngineAdapter {
    pub search_results: Arc<RwLock<Option<FullTextSearchResults>>>,
    pub suggestions: Arc<RwLock<Vec<String>>>,
    pub stats: Arc<RwLock<Option<SearchStats>>>,
    pub should_fail: bool,
}

impl MockSearchEngineAdapter {
    pub fn new() -> Self {
        Self {
            search_results: Arc::new(RwLock::new(None)),
            suggestions: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(None)),
            should_fail: false,
        }
    }
    
    pub fn with_results(mut self, results: FullTextSearchResults) -> Self {
        let mut search_results = self.search_results.write().unwrap();
        *search_results = Some(results);
        self
    }
    
    pub fn with_should_fail(mut self, should_fail: bool) -> Self {
        self.should_fail = should_fail;
        self
    }
}

#[async_trait]
impl SearchEnginePort for MockSearchEngineAdapter {
    async fn search(
        &self,
        _query: &FullTextSearchQuery,
    ) -> Result<FullTextSearchResults, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::SearchError("Mock search failure".to_string()));
        }
        
        let search_results = self.search_results.read().unwrap();
        match &*search_results {
            Some(results) => Ok(results.clone()),
            None => Ok(FullTextSearchResults::new(
                Vec::new(),
                0,
                1,
                20,
                10,
                0.0,
            )),
        }
    }
    
    async fn get_suggestions(
        &self,
        _partial_query: &str,
        _limit: usize,
    ) -> Result<Vec<String>, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::SearchError("Mock suggestions failure".to_string()));
        }
        
        let suggestions = self.suggestions.read().unwrap();
        Ok(suggestions.clone())
    }
    
    async fn get_stats(&self) -> Result<SearchStats, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::SearchError("Mock stats failure".to_string()));
        }
        
        let stats = self.stats.read().unwrap();
        match &*stats {
            Some(stats) => Ok(stats.clone()),
            None => Ok(SearchStats {
                total_documents: 0,
                total_terms: 0,
                average_document_length: 0.0,
                index_size_bytes: 0,
                last_indexed_at: None,
            }),
        }
    }
}

/// Mock indexer adapter for testing
#[derive(Debug, Clone)]
pub struct MockIndexerAdapter {
    pub indexed_artifacts: Arc<RwLock<Vec<IndexedArtifact>>>,
    pub should_fail: bool,
}

impl MockIndexerAdapter {
    pub fn new() -> Self {
        Self {
            indexed_artifacts: Arc::new(RwLock::new(Vec::new())),
            should_fail: false,
        }
    }
    
    pub fn with_should_fail(mut self, should_fail: bool) -> Self {
        self.should_fail = should_fail;
        self
    }
    
    pub fn get_indexed_artifacts(&self) -> Vec<IndexedArtifact> {
        let artifacts = self.indexed_artifacts.read().unwrap();
        artifacts.clone()
    }
}

#[async_trait]
impl IndexerPort for MockIndexerAdapter {
    async fn index_artifact(
        &self,
        artifact: &IndexedArtifact,
    ) -> Result<(), FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::IndexingError("Mock indexing failure".to_string()));
        }
        
        let mut artifacts = self.indexed_artifacts.write().unwrap();
        artifacts.push(artifact.clone());
        Ok(())
    }
    
    async fn index_artifacts_batch(
        &self,
        artifacts: &[IndexedArtifact],
    ) -> Result<BatchIndexingResult, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::BatchIndexingError("Mock batch indexing failure".to_string()));
        }
        
        let mut indexed_artifacts = self.indexed_artifacts.write().unwrap();
        indexed_artifacts.extend(artifacts.iter().cloned());
        
        Ok(BatchIndexingResult {
            indexed_count: artifacts.len(),
            failed_count: 0,
            errors: Vec::new(),
            duration_ms: 100,
        })
    }
    
    async fn delete_artifact(
        &self,
        _artifact_id: &str,
    ) -> Result<(), FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::IndexingError("Mock deletion failure".to_string()));
        }
        
        Ok(())
    }
    
    async fn reindex_all(&self) -> Result<ReindexingResult, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::BatchIndexingError("Mock reindexing failure".to_string()));
        }
        
        Ok(ReindexingResult {
            total_processed: 0,
            successful: 0,
            failed: 0,
            duration_ms: 100,
        })
    }
}

/// Mock tokenizer adapter for testing
#[derive(Debug, Clone)]
pub struct MockTokenizerAdapter {
    pub tokens: Arc<RwLock<Vec<Token>>>,
    pub language: Arc<RwLock<String>>,
    pub should_fail: bool,
}

impl MockTokenizerAdapter {
    pub fn new() -> Self {
        Self {
            tokens: Arc::new(RwLock::new(Vec::new())),
            language: Arc::new(RwLock::new("en".to_string())),
            should_fail: false,
        }
    }
    
    pub fn with_tokens(mut self, tokens: Vec<Token>) -> Self {
        let mut mock_tokens = self.tokens.write().unwrap();
        *mock_tokens = tokens;
        self
    }
    
    pub fn with_language(mut self, language: &str) -> Self {
        let mut mock_language = self.language.write().unwrap();
        *mock_language = language.to_string();
        self
    }
    
    pub fn with_should_fail(mut self, should_fail: bool) -> Self {
        self.should_fail = should_fail;
        self
    }
}

impl TokenizerPort for MockTokenizerAdapter {
    fn tokenize(&self, _text: &str) -> Result<Vec<Token>, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::TokenizerError("Mock tokenization failure".to_string()));
        }
        
        let tokens = self.tokens.read().unwrap();
        Ok(tokens.clone())
    }
    
    fn detect_language(&self, _text: &str) -> Result<String, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::LanguageDetectionError("Mock language detection failure".to_string()));
        }
        
        let language = self.language.read().unwrap();
        Ok(language.clone())
    }
    
    fn stem_tokens(&self, tokens: &[Token], _language: &str) -> Result<Vec<Token>, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::TokenizerError("Mock stemming failure".to_string()));
        }
        
        Ok(tokens.to_vec())
    }
}

/// Mock scorer adapter for testing
#[derive(Debug, Clone)]
pub struct MockScorerAdapter {
    pub scores: Arc<RwLock<Vec<f32>>>,
    pub should_fail: bool,
}

impl MockScorerAdapter {
    pub fn new() -> Self {
        Self {
            scores: Arc::new(RwLock::new(Vec::new())),
            should_fail: false,
        }
    }
    
    pub fn with_scores(mut self, scores: Vec<f32>) -> Self {
        let mut mock_scores = self.scores.write().unwrap();
        *mock_scores = scores;
        self
    }
    
    pub fn with_should_fail(mut self, should_fail: bool) -> Self {
        self.should_fail = should_fail;
        self
    }
}

impl ScorerPort for MockScorerAdapter {
    fn calculate_score(
        &self,
        _query_terms: &[String],
        _document_terms: &[String],
        _document_length: usize,
    ) -> Result<f32, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::ScoringError("Mock scoring failure".to_string()));
        }
        
        let scores = self.scores.read().unwrap();
        Ok(*scores.first().unwrap_or(&1.0))
    }
    
    fn normalize_scores(&self, scores: &[f32]) -> Result<Vec<f32>, FullTextSearchError> {
        if self.should_fail {
            return Err(FullTextSearchError::ScoringError("Mock normalization failure".to_string()));
        }
        
        Ok(scores.to_vec())
    }
}