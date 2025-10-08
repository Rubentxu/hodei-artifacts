use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegisterActionTypeError {
    #[error("Schema generation error: {0}")]
    SchemaGenerationError(String),

    #[error("Action type already registered: {0}")]
    DuplicateActionType(String),

    #[error("Invalid action type: {0}")]
    InvalidActionType(String),

    #[error("An unexpected internal error occurred: {0}")]
    InternalError(String),
}
