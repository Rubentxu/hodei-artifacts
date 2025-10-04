use thiserror::Error;

/// Errors specific to the evaluate permissions feature
#[derive(Debug, Error, Clone)]
pub enum EvaluatePermissionsError {
    #[error("Invalid authorization request: {0}")]
    InvalidRequest(String),

    #[error("Policy evaluation failed: {0}")]
    PolicyEvaluationFailed(String),

    #[error("IAM policy provider error: {0}")]
    IamPolicyProviderError(String),

    #[error("Organization boundary provider error: {0}")]
    OrganizationBoundaryProviderError(String),

    #[error("Cedar policy engine error: {0}")]
    CedarEngineError(String),

    #[error("Policy parsing error: {0}")]
    PolicyParsingError(String),

    #[error("Entity resolution error: {0}")]
    EntityResolutionError(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Timeout during authorization evaluation")]
    EvaluationTimeout,

    #[error("Internal authorization error: {0}")]
    InternalError(String),
}

impl From<crate::features::evaluate_permissions::ports::AuthorizationError>
    for EvaluatePermissionsError
{
    fn from(err: crate::features::evaluate_permissions::ports::AuthorizationError) -> Self {
        match err {
            crate::features::evaluate_permissions::ports::AuthorizationError::IamPolicyProvider(iam_err) => {
                EvaluatePermissionsError::IamPolicyProviderError(iam_err.to_string())
            }
            crate::features::evaluate_permissions::ports::AuthorizationError::OrganizationBoundaryProvider(msg) => {
                EvaluatePermissionsError::OrganizationBoundaryProviderError(msg)
            }
            crate::features::evaluate_permissions::ports::AuthorizationError::EntityResolver(entity_err) => {
                EvaluatePermissionsError::EntityResolutionError(entity_err.to_string())
            }
        }
    }
}

impl From<cedar_policy::PolicySetError> for EvaluatePermissionsError {
    fn from(err: cedar_policy::PolicySetError) -> Self {
        EvaluatePermissionsError::PolicyParsingError(err.to_string())
    }
}

impl From<cedar_policy::ValidationError> for EvaluatePermissionsError {
    fn from(err: cedar_policy::ValidationError) -> Self {
        EvaluatePermissionsError::PolicyParsingError(err.to_string())
    }
}

/// Result type for evaluate permissions operations
pub type EvaluatePermissionsResult<T> = Result<T, EvaluatePermissionsError>;
