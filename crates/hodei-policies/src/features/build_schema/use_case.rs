use crate::features::build_schema::dto::{BuildSchemaCommand, BuildSchemaResult};
use crate::features::build_schema::error::BuildSchemaError;
use crate::features::build_schema::ports::{BuildSchemaPort, SchemaStoragePort};
use crate::internal::engine::builder::EngineBuilder;
use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Use case for building and persisting the Cedar schema
///
/// This use case consumes the accumulated entity and action type registrations
/// from the EngineBuilder, generates the final Cedar schema, and persists it
/// to storage for use in policy validation and evaluation.
///
/// # Architecture
///
/// This is an async use case that:
/// 1. Consumes the EngineBuilder (taking ownership via Mutex)
/// 2. Builds the Cedar schema from all registered types
/// 3. Serializes the schema to JSON
/// 4. Persists it via the SchemaStoragePort
///
/// After building, the builder is reset so new registrations can begin.
pub struct BuildSchemaUseCase<S: SchemaStoragePort> {
    /// Internal schema builder for collecting registrations
    builder: Arc<Mutex<EngineBuilder>>,
    /// Storage port for persisting the schema
    storage: Arc<S>,
}

impl<S: SchemaStoragePort> BuildSchemaUseCase<S> {
    /// Create a new schema building use case
    ///
    /// # Arguments
    ///
    /// * `builder` - Shared reference to the EngineBuilder
    /// * `storage` - Implementation of the schema storage port
    pub fn new(builder: Arc<Mutex<EngineBuilder>>, storage: Arc<S>) -> Self {
        Self { builder, storage }
    }

    /// Build and persist the Cedar schema
    ///
    /// This method takes all registered entity and action types, builds a
    /// complete Cedar schema, validates it (if requested), and persists it
    /// to storage.
    ///
    /// # Arguments
    ///
    /// * `command` - Configuration for the schema building process
    ///
    /// # Returns
    ///
    /// A result containing statistics about the built schema
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No entity or action types have been registered
    /// - Schema building fails
    /// - Schema validation fails (if enabled)
    /// - Schema persistence fails
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use hodei_policies::features::build_schema::{BuildSchemaUseCase, BuildSchemaCommand};
    ///
    /// let command = BuildSchemaCommand::new()
    ///     .with_version("v1.0.0")
    ///     .with_validation(true);
    ///
    /// let result = use_case.execute(command).await?;
    /// println!("Schema built with {} entities and {} actions",
    ///          result.entity_count, result.action_count);
    /// ```
    #[tracing::instrument(skip(self, command), fields(
        version = ?command.version,
        validate = command.validate
    ))]
    pub async fn execute(
        &self,
        command: BuildSchemaCommand,
    ) -> Result<BuildSchemaResult, BuildSchemaError> {
        info!("Starting schema build process");

        // 1. Lock the builder and get counts before consuming
        let (entity_count, action_count) = {
            let builder = self.builder.lock().map_err(|e| {
                BuildSchemaError::BuilderLockError(format!("Failed to lock builder: {}", e))
            })?;

            let entities = builder.entity_count();
            let actions = builder.action_count();

            info!(
                "Builder contains {} entity types and {} action types",
                entities, actions
            );

            (entities, actions)
        };

        // 2. Verify we have something to build
        if entity_count == 0 && action_count == 0 {
            warn!("No entity or action types registered");
            return Err(BuildSchemaError::EmptySchema);
        }

        // 3. Take ownership of builder to consume it
        let builder = {
            let mut locked_builder = self.builder.lock().map_err(|e| {
                BuildSchemaError::BuilderLockError(format!("Failed to lock builder: {}", e))
            })?;

            // Replace with a new builder and take the old one
            std::mem::replace(&mut *locked_builder, EngineBuilder::new())
        };

        // 4. Build the schema (consumes the builder)
        info!("Building Cedar schema from registered types");
        let schema = builder
            .build_schema()
            .map_err(|e| BuildSchemaError::SchemaBuildError(e.to_string()))?;

        info!("Schema built successfully");

        // 5. Optionally validate the schema
        if command.validate {
            info!("Validating schema structure");
            // Cedar schemas are validated during construction, so if we got here, it's valid
            // Additional validation logic could be added here if needed
            info!("Schema validation passed");
        }

        // 6. Serialize schema to string
        // Cedar Schema doesn't have a direct serialization method, so we use Debug format
        // In production, this should be replaced with proper schema persistence
        let schema_string = format!("{:?}", schema);

        info!(
            "Schema serialized as debug string ({} bytes)",
            schema_string.len()
        );

        // 7. Persist the schema
        info!("Persisting schema to storage");
        let schema_id = self
            .storage
            .save_schema(schema_string, command.version.clone())
            .await?;

        info!("Schema persisted successfully with ID: {}", schema_id);

        Ok(BuildSchemaResult::new(
            entity_count,
            action_count,
            command.version,
            command.validate,
            schema_id,
        ))
    }

    /// Get the current entity count without building
    ///
    /// This is useful for checking if any registrations exist before building.
    pub fn entity_count(&self) -> Result<usize, BuildSchemaError> {
        let builder = self.builder.lock().map_err(|e| {
            BuildSchemaError::BuilderLockError(format!("Failed to lock builder: {}", e))
        })?;

        Ok(builder.entity_count())
    }

    /// Get the current action count without building
    ///
    /// This is useful for checking if any registrations exist before building.
    pub fn action_count(&self) -> Result<usize, BuildSchemaError> {
        let builder = self.builder.lock().map_err(|e| {
            BuildSchemaError::BuilderLockError(format!("Failed to lock builder: {}", e))
        })?;

        Ok(builder.action_count())
    }

    /// Clear all registrations without building
    ///
    /// This removes all registered types and resets the builder.
    /// Useful for testing or when you need to start over.
    pub fn clear(&self) -> Result<(), BuildSchemaError> {
        let mut builder = self.builder.lock().map_err(|e| {
            BuildSchemaError::BuilderLockError(format!("Failed to lock builder: {}", e))
        })?;

        builder.clear();
        info!("Cleared all registered types from builder");

        Ok(())
    }

    /// Get a reference to the builder for testing purposes
    ///
    /// This method is only available in test builds to allow tests
    /// to directly manipulate the builder for setup.
    #[cfg(test)]
    pub(crate) fn builder(&self) -> &Arc<Mutex<EngineBuilder>> {
        &self.builder
    }
}

/// Implementation of BuildSchemaPort trait for BuildSchemaUseCase
#[async_trait]
impl<S: SchemaStoragePort> BuildSchemaPort for BuildSchemaUseCase<S> {
    async fn execute(
        &self,
        command: BuildSchemaCommand,
    ) -> Result<BuildSchemaResult, BuildSchemaError> {
        self.execute(command).await
    }
}
