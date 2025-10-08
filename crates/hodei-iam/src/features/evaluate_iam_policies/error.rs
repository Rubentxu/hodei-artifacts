use thiserror::Error;

#[derive(Debug, Error)]
pub enum EvaluateIamPoliciesError {
    #[error("Policy not found: {0}")]
    PolicyNotFound(String),
    
    #[error("Principal not found: {0}")]
    PrincipalNotFound(String),
    
    #[error("Repository error: {0}")]
    RepositoryError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
}
