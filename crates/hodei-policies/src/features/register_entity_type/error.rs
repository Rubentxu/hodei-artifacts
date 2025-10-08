use thiserror::Error;

#[derive(Debug, Error)]
pub enum RegisterEntityTypeError {
    #[error("Schema generation error: {0}")]
    SchemaGenerationError(String),

    #[error("Entity type already registered: {0}")]
    DuplicateEntityType(String),

    #[error("Invalid entity type: {0}")]
    InvalidEntityType(String),

    #[error("An unexpected internal error occurred: {0}")]
    InternalError(String),
}
