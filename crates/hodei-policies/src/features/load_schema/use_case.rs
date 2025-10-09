use crate::features::load_schema::dto::{LoadSchemaCommand, LoadSchemaResult};
use crate::features::load_schema::error::LoadSchemaError;
use crate::features::load_schema::ports::{LoadSchemaPort, SchemaStoragePort};
use async_trait::async_trait;
use cedar_policy::Schema;
use std::sync::Arc;
use tracing::{info, warn};

/// Use case for loading Cedar schemas from storage
///
/// This use case retrieves previously built and persisted schemas
/// from storage, making them available for policy validation and evaluation.
///
/// # Architecture
///
/// This is an async use case that:
/// 1. Queries the storage for a specific or latest schema
/// 2. Retrieves the schema string representation
/// 3. Parses it back into a Cedar Schema object
/// 4. Returns the schema for use by other features
pub struct LoadSchemaUseCase<S: SchemaStoragePort> {
    /// Storage port for retrieving schemas
    storage: Arc<S>,
}

impl<S: SchemaStoragePort> LoadSchemaUseCase<S> {
    /// Create a new schema loading use case
    ///
    /// # Arguments
    ///
    /// * `storage` - Implementation of the schema storage port
    pub fn new(storage: Arc<S>) -> Self {
        Self { storage }
    }

    /// Load a Cedar schema from storage
    ///
    /// This method retrieves a schema from storage, either by version
    /// or loading the latest available schema.
    ///
    /// # Arguments
    ///
    /// * `command` - Configuration specifying which schema to load
    ///
    /// # Returns
    ///
    /// The loaded schema along with its version and identifier
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The requested schema version doesn't exist
    /// - No schemas are available in storage
    /// - The storage backend is unavailable
    /// - Schema parsing fails
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_policies::features::load_schema::{LoadSchemaUseCase, LoadSchemaCommand};
    ///
    /// // Load the latest schema
    /// let command = LoadSchemaCommand::latest();
    /// let result = use_case.execute(command).await?;
    ///
    /// // Load a specific version
    /// let command = LoadSchemaCommand::with_version("v1.0.0");
    /// let result = use_case.execute(command).await?;
    /// ```
    #[tracing::instrument(skip(self, command), fields(
        version = ?command.version
    ))]
    pub async fn execute(
        &self,
        command: LoadSchemaCommand,
    ) -> Result<LoadSchemaResult, LoadSchemaError> {
        if let Some(ref version) = command.version {
            info!("Loading schema version: {}", version);
            self.load_by_version(version).await
        } else {
            info!("Loading latest schema version");
            self.load_latest().await
        }
    }

    /// Load a specific schema version
    async fn load_by_version(&self, version: &str) -> Result<LoadSchemaResult, LoadSchemaError> {
        let schema_string = self
            .storage
            .get_schema_by_version(version)
            .await?
            .ok_or_else(|| {
                LoadSchemaError::InvalidSchemaVersion(format!(
                    "Schema version '{}' not found",
                    version
                ))
            })?;

        info!("Retrieved schema version '{}' from storage", version);

        // Parse the schema string back into a Schema object
        // Note: Currently we're using debug format, so parsing might not work
        // This is a placeholder for when proper serialization is implemented
        let schema = self.parse_schema(&schema_string)?;

        info!("Successfully loaded schema version '{}'", version);

        Ok(LoadSchemaResult::new(
            schema,
            Some(version.to_string()),
            format!("schema_{}", version),
        ))
    }

    /// Load the latest schema version
    async fn load_latest(&self) -> Result<LoadSchemaResult, LoadSchemaError> {
        let schema_string = self
            .storage
            .get_latest_schema()
            .await?
            .ok_or(LoadSchemaError::SchemaNotFound)?;

        info!("Retrieved latest schema from storage");

        // Parse the schema string back into a Schema object
        let schema = self.parse_schema(&schema_string)?;

        info!("Successfully loaded latest schema");

        Ok(LoadSchemaResult::new(
            schema,
            None,
            "schema_latest".to_string(),
        ))
    }

    /// Parse a schema string into a Cedar Schema object
    ///
    /// This method attempts to parse the stored schema representation
    /// back into a Cedar Schema. Currently this is a placeholder since
    /// we're using debug format for storage.
    fn parse_schema(&self, _schema_string: &str) -> Result<Schema, LoadSchemaError> {
        warn!("Schema parsing from debug format is not yet implemented");
        warn!("Returning empty schema as placeholder");

        // TODO: Implement proper schema deserialization
        // For now, we return an empty schema to allow the system to work
        // In production, this should properly deserialize the stored schema
        Schema::from_schema_fragments(vec![])
            .map_err(|e| LoadSchemaError::SchemaParsingError(e.to_string()))
    }

    /// List all available schema versions
    ///
    /// This is a utility method to help discover what schemas are available.
    pub async fn list_versions(&self) -> Result<Vec<String>, LoadSchemaError> {
        self.storage
            .list_schema_versions()
            .await
            .map_err(|e| LoadSchemaError::SchemaStorageError(e.to_string()))
    }
}

/// Implementation of LoadSchemaPort trait for LoadSchemaUseCase
#[async_trait]
impl<S: SchemaStoragePort> LoadSchemaPort for LoadSchemaUseCase<S> {
    async fn execute(
        &self,
        command: LoadSchemaCommand,
    ) -> Result<LoadSchemaResult, LoadSchemaError> {
        self.execute(command).await
    }
}
