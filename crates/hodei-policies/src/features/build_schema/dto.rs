//! Data Transfer Objects for the build_schema feature
//!
//! This module defines the input and output DTOs for the schema building process.

/// Command to build the Cedar schema
///
/// This command triggers the schema building process, consuming all
/// registered entity and action types and generating the final Cedar schema.
#[derive(Debug, Clone)]
pub struct BuildSchemaCommand {
    /// Schema version identifier (optional)
    pub version: Option<String>,

    /// Whether to validate the schema after building
    pub validate: bool,
}

impl Default for BuildSchemaCommand {
    fn default() -> Self {
        Self {
            version: None,
            validate: true,
        }
    }
}

impl BuildSchemaCommand {
    /// Create a new build schema command with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the schema version
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = Some(version.into());
        self
    }

    /// Set whether to validate the schema after building
    pub fn with_validation(mut self, validate: bool) -> Self {
        self.validate = validate;
        self
    }
}

/// Result of the schema building operation
#[derive(Debug, Clone)]
pub struct BuildSchemaResult {
    /// Number of entity types included in the schema
    pub entity_count: usize,

    /// Number of action types included in the schema
    pub action_count: usize,

    /// Schema version identifier (if provided)
    pub version: Option<String>,

    /// Whether the schema was validated
    pub validated: bool,

    /// Schema ID or identifier in storage
    pub schema_id: String,
}

impl BuildSchemaResult {
    /// Create a new build schema result
    pub fn new(
        entity_count: usize,
        action_count: usize,
        version: Option<String>,
        validated: bool,
        schema_id: String,
    ) -> Self {
        Self {
            entity_count,
            action_count,
            version,
            validated,
            schema_id,
        }
    }
}
