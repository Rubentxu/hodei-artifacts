use thiserror::Error;

/// Errores específicos del caso de uso GetEffectivePoliciesForPrincipal
#[derive(Debug, Error)]
pub enum GetEffectivePoliciesError {
    #[error("Principal not found: {0}")]
    PrincipalNotFound(String),

    #[error("Invalid principal HRN: {0}")]
    InvalidPrincipalHrn(String),

    #[error("Invalid principal type: {0}. Expected 'user' or 'service-account'")]
    InvalidPrincipalType(String),

    #[error("Group not found: {0}")]
    GroupNotFound(String),

    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    #[error("Failed to parse policy document: {0}")]
    PolicyParseError(String),

    #[error("Repository error: {0}")]
    RepositoryError(String),

    #[error("Internal error: {0}")]
    InternalError(String),
}

/// Tipo Result específico para este caso de uso
pub type GetEffectivePoliciesResult<T> = Result<T, GetEffectivePoliciesError>;
