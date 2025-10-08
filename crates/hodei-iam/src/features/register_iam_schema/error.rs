//! Error types for the register_iam_schema feature
//!
//! This module defines the errors that can occur during IAM schema registration.

use thiserror::Error;

/// Errors that can occur during IAM schema registration
#[derive(Debug, Error)]
pub enum RegisterIamSchemaError {
    /// Error registering an entity type
    #[error("Failed to register entity type: {0}")]
    EntityTypeRegistrationError(String),

    /// Error registering an action type
    #[error("Failed to register action type: {0}")]
    ActionTypeRegistrationError(String),

    /// Error building the schema
    #[error("Failed to build schema: {0}")]
    SchemaBuildError(String),

    /// Error during schema validation
    #[error("Schema validation failed: {0}")]
    SchemaValidationError(String),

    /// No entity or action types were registered
    #[error("No entity or action types registered before schema build")]
    NoTypesRegistered,

    /// An unexpected internal error occurred
    #[error("Internal error during schema registration: {0}")]
    InternalError(String),
}
