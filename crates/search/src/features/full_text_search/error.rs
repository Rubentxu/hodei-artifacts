use thiserror::Error;
use std::fmt;

/// Custom error types for full-text search functionality
#[derive(Error, Debug)]
pub enum FullTextSearchError {
    #[error("Failed to index artifact: {0}")]
    IndexingError(String),
    
    #[error("Failed to search artifacts: {0}")]
    SearchError(String),
    
    #[error("Invalid search query: {0}")]
    InvalidQueryError(String),
    
    #[error("Tokenizer configuration error: {0}")]
    TokenizerError(String),
    
    #[error("Scoring calculation error: {0}")]
    ScoringError(String),
    
    #[error("Index schema error: {0}")]
    SchemaError(String),
    
    #[error("Batch indexing error: {0}")]
    BatchIndexingError(String),
    
    #[error("Language detection error: {0}")]
    LanguageDetectionError(String),
    
    #[error("Query parsing error: {0}")]
    QueryParsingError(String),
    
    #[error("Internal search engine error: {0}")]
    InternalError(String),
}

impl fmt::Display for FullTextSearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FullTextSearchError::IndexingError(msg) => write!(f, "Failed to index artifact: {}", msg),
            FullTextSearchError::SearchError(msg) => write!(f, "Failed to search artifacts: {}", msg),
            FullTextSearchError::InvalidQueryError(msg) => write!(f, "Invalid search query: {}", msg),
            FullTextSearchError::TokenizerError(msg) => write!(f, "Tokenizer configuration error: {}", msg),
            FullTextSearchError::ScoringError(msg) => write!(f, "Scoring calculation error: {}", msg),
            FullTextSearchError::SchemaError(msg) => write!(f, "Index schema error: {}", msg),
            FullTextSearchError::BatchIndexingError(msg) => write!(f, "Batch indexing error: {}", msg),
            FullTextSearchError::LanguageDetectionError(msg) => write!(f, "Language detection error: {}", msg),
            FullTextSearchError::QueryParsingError(msg) => write!(f, "Query parsing error: {}", msg),
            FullTextSearchError::InternalError(msg) => write!(f, "Internal search engine error: {}", msg),
        }
    }
}