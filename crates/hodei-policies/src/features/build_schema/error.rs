use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildSchemaError {
    #[error("Schema building error: {0}")]
    SchemaBuildError(String),

    #[error("Schema storage error: {0}")]
    SchemaStorageError(String),

    #[error("No entity or action types registered")]
    EmptySchema,

    #[error("Schema validation error: {0}")]
    SchemaValidationError(String),

    #[error("Builder lock error: {0}")]
    BuilderLockError(String),

    #[error("An unexpected internal error occurred: {0}")]
    InternalError(String),
}
