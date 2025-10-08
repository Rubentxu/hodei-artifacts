use crate::features::build_schema::error::BuildSchemaError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadSchemaError {
    #[error("Schema not found")]
    SchemaNotFound,

    #[error("Schema storage error: {0}")]
    SchemaStorageError(String),

    #[error("Schema parsing error: {0}")]
    SchemaParsingError(String),

    #[error("Invalid schema version: {0}")]
    InvalidSchemaVersion(String),

    #[error("Schema deserialization error: {0}")]
    SchemaDeserializationError(String),

    #[error("An unexpected internal error occurred: {0}")]
    InternalError(String),
}

impl From<BuildSchemaError> for LoadSchemaError {
    fn from(error: BuildSchemaError) -> Self {
        LoadSchemaError::SchemaStorageError(error.to_string())
    }
}
