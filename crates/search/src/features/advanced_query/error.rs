use thiserror::Error;
use std::fmt;

#[derive(Error, Debug)]
pub enum AdvancedQueryError {
    #[error("Query parse error: {0}")]
    QueryParseError(String),
    
    #[error("Invalid field error: {0}")]
    InvalidFieldError(String),
    
    #[error("Invalid range error: {0}")]
    InvalidRangeError(String),
    
    #[error("Invalid boolean operator error: {0}")]
    InvalidBooleanOperatorError(String),
    
    #[error("Unmatched parentheses error")]
    UnmatchedParenthesesError,
    
    #[error("Query too complex error")]
    QueryTooComplexError,
    
    #[error("Query timeout error")]
    QueryTimeoutError,
    
    #[error("Search execution error: {0}")]
    SearchExecutionError(String),
    
    #[error("Internal error: {0}")]
    InternalError(String),
}

// Note: We don't need to implement Display manually since we're using thiserror
// which automatically implements it for us based on the #[error(...)] attributes