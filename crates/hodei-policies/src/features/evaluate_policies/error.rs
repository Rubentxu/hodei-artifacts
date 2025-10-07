use thiserror::Error;

#[derive(Debug, Error)]
pub enum EvaluatePoliciesError {
    #[error("An unexpected internal error occurred: {0}")]
    InternalError(String),
    #[error("Failed to translate HRN to Cedar EntityUid: {0}")]
    TranslationError(String),
    #[error("Schema building failed: {0}")]
    SchemaError(String),
}
