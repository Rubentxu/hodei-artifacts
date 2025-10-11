//! Data Transfer Objects for the register_iam_schema feature
//!
//! This module defines the input and output DTOs for the IAM schema registration process.

use kernel::domain::entity::ActionTrait;
use kernel::domain::value_objects::ServiceName;
use serde::{Deserialize, Serialize};

/// Command to register the IAM schema
///
/// This command triggers the registration of all IAM entity types and action types
/// with the policies engine, followed by schema building and persistence.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct RegisterIamSchemaCommand {
    /// Optional specific version identifier for the schema
    /// If None, a timestamp-based version will be generated
    pub version: Option<String>,

    /// Whether to validate the schema after building
    pub validate: bool,
}

impl ActionTrait for RegisterIamSchemaCommand {
    fn name() -> &'static str {
        "RegisterIamSchema"
    }

    fn service_name() -> ServiceName {
        ServiceName::new("iam").expect("Valid service name")
    }

    fn applies_to_principal() -> String {
        "Iam::User".to_string()
    }

    fn applies_to_resource() -> String {
        "Iam::Schema".to_string()
    }
}

impl RegisterIamSchemaCommand {
    /// Create a new register IAM schema command with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set a specific schema version
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set whether to validate the schema after building
    pub fn with_validation(mut self, validate: bool) -> Self {
        self.validate = validate;
        self
    }

    /// Disable schema validation (default is enabled)
    pub fn without_validation(mut self) -> Self {
        self.validate = false;
        self
    }
}

/// Result of the IAM schema registration operation
#[derive(Debug, Clone)]
pub struct RegisterIamSchemaResult {
    /// Number of entity types registered
    pub entity_types_registered: usize,

    /// Number of action types registered
    pub action_types_registered: usize,

    /// The schema version identifier
    pub schema_version: String,

    /// Schema ID in storage
    pub schema_id: String,

    /// Whether the schema was validated
    pub validated: bool,
}

impl RegisterIamSchemaResult {
    /// Create a new registration result
    pub fn new(
        entity_types_registered: usize,
        action_types_registered: usize,
        schema_version: String,
        schema_id: String,
        validated: bool,
    ) -> Self {
        Self {
            entity_types_registered,
            action_types_registered,
            schema_version,
            schema_id,
            validated,
        }
    }
}
