use thiserror::Error;
use std::fmt;

#[derive(Error, Debug)]
pub enum FullTextSearchError {
    #[error("Search index error: {0}")]
    SearchIndexError(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
    
    #[error("Event publishing error: {0}")]
    EventPublishingError(String),
    
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    
    #[error("Search timeout error")]
    TimeoutError,
    
    #[error("Authentication error: {0}")]
    AuthenticationError(String),
    
    #[error("Query too complex error")]
    QueryTooComplexError,
    
    #[error("Unmatched parentheses error")]
    UnmatchedParenthesesError,
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

// Note: We don't need to implement Display manually since we're using thiserror
// which automatically implements it for us based on the #[error(...)] attributes