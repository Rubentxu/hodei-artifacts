//! Data Transfer Objects for the load_schema feature
//!
//! This module defines the input and output DTOs for the schema loading process.

use cedar_policy::Schema;

/// Command to load a Cedar schema from storage
///
/// This command specifies which schema version to load, or defaults
/// to the latest if no version is specified.
#[derive(Debug, Clone, Default)]
pub struct LoadSchemaCommand {
    /// Optional specific version to load. If None, loads the latest.
    pub version: Option<String>,
}

impl LoadSchemaCommand {
    /// Create a new load schema command that loads the latest version
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a command to load a specific schema version
    pub fn with_version(version: impl Into<String>) -> Self {
        Self {
            version: Some(version.into()),
        }
    }

    /// Load the latest schema version
    pub fn latest() -> Self {
        Self { version: None }
    }
}

/// Result of the schema loading operation
#[derive(Debug)]
pub struct LoadSchemaResult {
    /// The loaded Cedar schema
    pub schema: Schema,

    /// The version identifier of the loaded schema
    pub version: Option<String>,

    /// Schema ID or identifier in storage
    pub schema_id: String,
}

impl LoadSchemaResult {
    /// Create a new load schema result
    pub fn new(schema: Schema, version: Option<String>, schema_id: String) -> Self {
        Self {
            schema,
            version,
            schema_id,
        }
    }
}
