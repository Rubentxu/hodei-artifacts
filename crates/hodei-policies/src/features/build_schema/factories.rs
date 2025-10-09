//! Factory functions for the build_schema feature
//!
//! This module provides static factory functions following the Java Config pattern.
//! Factories receive already-constructed dependencies and assemble use cases.

use crate::features::build_schema::ports::{BuildSchemaPort, SchemaStoragePort};
use crate::features::build_schema::use_case::BuildSchemaUseCase;
use crate::features::register_action_type::RegisterActionTypeUseCase;
use crate::features::register_action_type::ports::RegisterActionTypePort;
use crate::features::register_entity_type::RegisterEntityTypeUseCase;
use crate::features::register_entity_type::ports::RegisterEntityTypePort;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::{Arc, Mutex};

/// Creates a complete bundle of schema registration components
///
/// This factory receives an already-constructed schema storage implementation
/// and assembles all the use cases needed for schema registration workflows.
/// It internally creates and shares an EngineBuilder among the use cases.
///
/// # Arguments
///
/// * `storage` - Pre-constructed implementation of SchemaStoragePort
///
/// # Returns
///
/// A tuple containing:
/// * `Arc<dyn RegisterEntityTypePort>` - Port for registering entity types
/// * `Arc<dyn RegisterActionTypePort>` - Port for registering action types
/// * `Arc<dyn BuildSchemaPort>` - Port for building and persisting schemas
///
/// # Example
///
/// ```rust,ignore
/// use hodei_policies::features::build_schema::factories;
/// use std::sync::Arc;
///
/// // Composition root creates the adapter
/// let schema_storage = Arc::new(SurrealSchemaStorage::new(db_client));
///
/// // Factory receives the adapter and assembles the use cases
/// let (entity_uc, action_uc, schema_uc) =
///     factories::create_schema_registration_components(schema_storage);
///
/// // Use the components for schema registration
/// entity_uc.execute(register_entity_cmd).await?;
/// action_uc.execute(register_action_cmd).await?;
/// schema_uc.execute(build_schema_cmd).await?;
/// ```
pub fn create_schema_registration_components<S: SchemaStoragePort + 'static>(
    storage: Arc<S>,
) -> (
    Arc<dyn RegisterEntityTypePort>,
    Arc<dyn RegisterActionTypePort>,
    Arc<dyn BuildSchemaPort>,
) {
    // Create shared EngineBuilder (internal coordination)
    let builder = Arc::new(Mutex::new(EngineBuilder::new()));

    // Assemble the three use cases with shared builder
    let entity_uc: Arc<dyn RegisterEntityTypePort> =
        Arc::new(RegisterEntityTypeUseCase::new(builder.clone()));
    let action_uc: Arc<dyn RegisterActionTypePort> =
        Arc::new(RegisterActionTypeUseCase::new(builder.clone()));
    let schema_uc: Arc<dyn BuildSchemaPort> = Arc::new(BuildSchemaUseCase::new(builder, storage));

    (entity_uc, action_uc, schema_uc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::build_schema::error::BuildSchemaError;
    use async_trait::async_trait;

    /// Simple in-memory mock storage for testing
    struct MockSchemaStorage;

    #[async_trait]
    impl SchemaStoragePort for MockSchemaStorage {
        async fn save_schema(
            &self,
            _schema_json: String,
            _version: Option<String>,
        ) -> Result<String, BuildSchemaError> {
            Ok("mock-schema-id".to_string())
        }

        async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
            Ok(None)
        }

        async fn get_schema_by_version(
            &self,
            _version: &str,
        ) -> Result<Option<String>, BuildSchemaError> {
            Ok(None)
        }

        async fn delete_schema(&self, _schema_id: &str) -> Result<bool, BuildSchemaError> {
            Ok(false)
        }

        async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
            Ok(vec![])
        }
    }

    #[test]
    fn test_create_schema_registration_components_returns_all_components() {
        let storage = Arc::new(MockSchemaStorage);

        // Call the factory
        let (entity_uc, action_uc, schema_uc) = create_schema_registration_components(storage);

        // Verify that all components are properly created
        assert!(Arc::strong_count(&entity_uc) >= 1);
        assert!(Arc::strong_count(&action_uc) >= 1);
        assert!(Arc::strong_count(&schema_uc) >= 1);
    }

    #[test]
    fn test_factory_does_not_expose_internal_details() {
        let storage = Arc::new(MockSchemaStorage);

        // This test verifies that the factory can be used
        // without any knowledge of EngineBuilder internals
        let (_entity_uc, _action_uc, _schema_uc) = create_schema_registration_components(storage);

        // If this compiles, it proves that internal details are properly encapsulated
    }
}
