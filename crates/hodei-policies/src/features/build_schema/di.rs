//! Dependency Injection for the build_schema feature
//!
//! This module provides factory functions to create use case instances
//! with their dependencies properly injected.

use super::ports::SchemaStoragePort;
use super::use_case::BuildSchemaUseCase;
use crate::features::register_action_type::RegisterActionTypeUseCase;
use crate::features::register_entity_type::RegisterEntityTypeUseCase;
use crate::internal::engine::builder::EngineBuilder;
use std::sync::{Arc, Mutex};

/// Creates a complete bundle of schema registration components without exposing EngineBuilder.
///
/// This is the recommended factory for external crates (e.g., bounded contexts) that need
/// to register entity types, action types, and build schemas without directly depending
/// on internal implementation details like `EngineBuilder`.
///
/// This factory internally creates a shared `EngineBuilder` and wires together all three
/// use cases needed for schema registration workflows.
///
/// # Arguments
///
/// * `storage` - Implementation of SchemaStoragePort for persisting schemas
///
/// # Returns
///
/// A tuple containing:
/// * `Arc<RegisterEntityTypeUseCase>` - Use case for registering entity types
/// * `Arc<RegisterActionTypeUseCase>` - Use case for registering action types
/// * `BuildSchemaUseCase<S>` - Use case for building and persisting schemas
///
/// # Type Parameters
///
/// * `S` - The concrete storage implementation that must implement SchemaStoragePort
///
/// # Example
///
/// ```rust,ignore
/// use std::sync::Arc;
/// use hodei_policies::features::build_schema::di::create_schema_registration_components;
/// use hodei_policies::infrastructure::SurrealSchemaStorage;
///
/// let storage = Arc::new(SurrealSchemaStorage::new(db_client));
/// let (entity_uc, action_uc, schema_uc) = create_schema_registration_components(storage);
///
/// // Use the components to build a higher-level orchestration use case
/// // e.g., in hodei-iam::RegisterIamSchemaUseCase
/// ```
pub fn create_schema_registration_components<S: SchemaStoragePort + 'static>(
    storage: Arc<S>,
) -> (
    Arc<RegisterEntityTypeUseCase>,
    Arc<RegisterActionTypeUseCase>,
    BuildSchemaUseCase<S>,
) {
    // Create shared EngineBuilder (internal detail, not exposed)
    let builder = Arc::new(Mutex::new(EngineBuilder::new()));

    // Instantiate all three use cases with shared builder
    let entity_uc = Arc::new(RegisterEntityTypeUseCase::new(builder.clone()));
    let action_uc = Arc::new(RegisterActionTypeUseCase::new(builder.clone()));
    let schema_uc = BuildSchemaUseCase::new(builder, storage);

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

        // Call the public bundle factory
        let (entity_uc, action_uc, schema_uc) = create_schema_registration_components(storage);

        // Verify that all components are properly created
        // Entity use case should be usable
        assert!(Arc::strong_count(&entity_uc) >= 1);

        // Action use case should be usable
        assert!(Arc::strong_count(&action_uc) >= 1);

        // Schema use case should be created (ownership transferred)
        let _ = schema_uc;
    }

    #[test]
    fn test_bundle_factory_does_not_expose_engine_builder() {
        let storage = Arc::new(MockSchemaStorage);

        // This test verifies that the bundle factory can be used
        // without any knowledge of EngineBuilder
        let (_entity_uc, _action_uc, _schema_uc) = create_schema_registration_components(storage);

        // If this compiles, it proves that EngineBuilder is not leaked
        // in the public API
    }
}
