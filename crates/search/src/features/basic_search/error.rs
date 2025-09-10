use thiserror::Error;

#[derive(Error, Debug)]
pub enum BasicSearchError {
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
}