use thiserror::Error;
use std::fmt;

#[derive(Error, Debug)]
pub enum AdvancedQueryError {
    #[error("Failed to parse query: {0}")]
    QueryParseError(String),
    
    #[error("Invalid field name: {0}")]
    InvalidFieldError(String),
    
    #[error("Invalid range query: {0}")]
    InvalidRangeError(String),
    
    #[error("Invalid boolean operator: {0}")]
    InvalidBooleanOperatorError(String),
    
    #[error("Unmatched parentheses in query")]
    UnmatchedParenthesesError,
    
    #[error("Query too complex: exceeded maximum nesting depth")]
    QueryTooComplexError,
    
    #[error("Query parsing timeout exceeded")]
    QueryTimeoutError,
    
    #[error("Internal query processing error: {0}")]
    InternalError(String),
}

impl fmt::Display for AdvancedQueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdvancedQueryError::QueryParseError(msg) => write!(f, "Failed to parse query: {}", msg),
            AdvancedQueryError::InvalidFieldError(field) => write!(f, "Invalid field name: {}", field),
            AdvancedQueryError::InvalidRangeError(range) => write!(f, "Invalid range query: {}", range),
            AdvancedQueryError::InvalidBooleanOperatorError(op) => write!(f, "Invalid boolean operator: {}", op),
            AdvancedQueryError::UnmatchedParenthesesError => write!(f, "Unmatched parentheses in query"),
            AdvancedQueryError::QueryTooComplexError => write!(f, "Query too complex: exceeded maximum nesting depth"),
            AdvancedQueryError::QueryTimeoutError => write!(f, "Query parsing timeout exceeded"),
            AdvancedQueryError::InternalError(msg) => write!(f, "Internal query processing error: {}", msg),
        }
    }
}