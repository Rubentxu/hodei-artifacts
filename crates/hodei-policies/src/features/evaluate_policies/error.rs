use thiserror::Error;

#[derive(Debug, Error)]
pub enum EvaluatePoliciesError {
    #[error("Policy load error: {0}")]
    PolicyLoadError(String),

    #[error("Entity registration error: {0}")]
    EntityRegistrationError(String),

    #[error("Policy evaluation error: {0}")]
    EvaluationError(String),

    #[error("Cache clear error: {0}")]
    CacheClearError(String),

    #[error("An unexpected internal error occurred: {0}")]
    InternalError(String),

    #[error("Failed to translate HRN to Cedar EntityUid: {0}")]
    TranslationError(String),

    #[error("Schema building failed: {0}")]
    SchemaError(String),

    #[error("Schema not found: {0}")]
    SchemaNotFound(String),

    #[error("Schema loading failed: {0}")]
    SchemaLoadError(String),

    #[error("Strict mode requires schema but none was found")]
    StrictModeSchemaRequired,
}
