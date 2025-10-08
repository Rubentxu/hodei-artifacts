//! Ports (trait definitions) for the build_schema feature
//!
//! This module defines the public interfaces that the BuildSchemaUseCase
//! depends on. These traits enable dependency inversion and testability.

use async_trait::async_trait;
use cedar_policy::Schema;

use crate::features::build_schema::error::BuildSchemaError;

/// Stored schema data retrieved from storage
#[derive(Debug, Clone)]
pub struct StoredSchema {
    /// The schema serialized as a string (debug format for now)
    pub schema_string: String,

    /// Optional version identifier
    pub version: Option<String>,

    /// Schema ID in storage
    pub schema_id: String,
}

impl StoredSchema {
    /// Create a new stored schema
    pub fn new(schema_string: String, version: Option<String>, schema_id: String) -> Self {
        Self {
            schema_string,
            version,
            schema_id,
        }
    }

    /// Parse the schema string into a Cedar Schema object
    ///
    /// Note: Currently returns an empty schema as a placeholder
    /// since we're using debug format for storage.
    pub fn parse(&self) -> Result<Schema, BuildSchemaError> {
        // TODO: Implement proper schema deserialization
        // For now, return an empty schema to allow the system to work
        Schema::from_schema_fragments(vec![])
            .map_err(|e| BuildSchemaError::SchemaBuildError(e.to_string()))
    }
}

/// Port for schema storage operations
///
/// This trait defines the contract for persisting and retrieving Cedar schemas.
/// It allows the build_schema use case to remain agnostic to the actual
/// storage implementation (SurrealDB, file system, in-memory, etc.).
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::build_schema::ports::SchemaStoragePort;
///
/// async fn save_schema(
///     storage: &dyn SchemaStoragePort,
///     schema_json: String,
///     version: Option<String>
/// ) -> Result<String, BuildSchemaError> {
///     storage.save_schema(schema_json, version).await
/// }
/// ```
#[async_trait]
pub trait SchemaStoragePort: Send + Sync {
    /// Save a Cedar schema to storage
    ///
    /// This method persists the schema and returns a unique identifier
    /// that can be used to retrieve it later.
    ///
    /// # Arguments
    ///
    /// * `schema_json` - The schema serialized as JSON
    /// * `version` - Optional version identifier for the schema
    ///
    /// # Returns
    ///
    /// A unique identifier for the stored schema
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The storage backend is unavailable
    /// - The schema fails to serialize
    /// - A constraint violation occurs
    async fn save_schema(
        &self,
        schema_json: String,
        version: Option<String>,
    ) -> Result<String, BuildSchemaError>;

    /// Retrieve the latest schema from storage
    ///
    /// This method fetches the most recently saved schema.
    ///
    /// # Returns
    ///
    /// The schema as a JSON string, or None if no schema exists
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The storage backend is unavailable
    /// - The schema fails to deserialize
    async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError>;

    /// Retrieve a specific schema version from storage
    ///
    /// # Arguments
    ///
    /// * `version` - The version identifier of the schema to retrieve
    ///
    /// # Returns
    ///
    /// The schema as a JSON string, or None if the version doesn't exist
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The storage backend is unavailable
    /// - The schema fails to deserialize
    async fn get_schema_by_version(
        &self,
        version: &str,
    ) -> Result<Option<String>, BuildSchemaError>;

    /// Delete a schema by its identifier
    ///
    /// # Arguments
    ///
    /// * `schema_id` - The unique identifier of the schema to delete
    ///
    /// # Returns
    ///
    /// True if the schema was deleted, false if it didn't exist
    ///
    /// # Errors
    ///
    /// Returns an error if the storage backend is unavailable
    async fn delete_schema(&self, schema_id: &str) -> Result<bool, BuildSchemaError>;

    /// List all available schema versions
    ///
    /// # Returns
    ///
    /// A vector of schema identifiers or version strings
    ///
    /// # Errors
    ///
    /// Returns an error if the storage backend is unavailable
    async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError>;

    /// Load a schema from storage (either latest or specific version)
    ///
    /// This is a convenience method that combines get_latest_schema
    /// and get_schema_by_version into a single operation.
    ///
    /// # Arguments
    ///
    /// * `version` - Optional version identifier. If None, loads the latest schema.
    ///
    /// # Returns
    ///
    /// A StoredSchema containing the schema string, version, and ID
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The requested version doesn't exist
    /// - No schemas are available (when loading latest)
    /// - The storage backend is unavailable
    async fn load_schema(&self, version: Option<String>) -> Result<StoredSchema, BuildSchemaError> {
        if let Some(ver) = version {
            let schema_string = self.get_schema_by_version(&ver).await?.ok_or_else(|| {
                BuildSchemaError::SchemaStorageError(format!("Schema version '{}' not found", ver))
            })?;

            Ok(StoredSchema::new(
                schema_string,
                Some(ver.clone()),
                format!("schema_{}", ver),
            ))
        } else {
            let schema_string = self.get_latest_schema().await?.ok_or_else(|| {
                BuildSchemaError::SchemaStorageError("No schemas found in storage".to_string())
            })?;

            Ok(StoredSchema::new(
                schema_string,
                None,
                "schema_latest".to_string(),
            ))
        }
    }
}
