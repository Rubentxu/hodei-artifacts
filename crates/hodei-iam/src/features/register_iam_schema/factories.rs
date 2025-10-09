//! Factory functions for the register_iam_schema feature
//!
//! This module provides static factory functions following the Java Config pattern.
//! Factories receive already-constructed dependencies and assemble use cases.

use crate::features::register_iam_schema::ports::RegisterIamSchemaPort;
use crate::features::register_iam_schema::use_case::RegisterIamSchemaUseCase;
use hodei_policies::build_schema::ports::{BuildSchemaPort, SchemaStoragePort};
use hodei_policies::register_action_type::ports::RegisterActionTypePort;
use hodei_policies::register_entity_type::ports::RegisterEntityTypePort;
use std::sync::Arc;
use tracing::debug;

/// Creates a RegisterIamSchemaUseCase from pre-constructed dependencies
///
/// This factory receives already-constructed port implementations from hodei-policies
/// and assembles the IAM schema registration use case.
///
/// # Arguments
///
/// * `entity_type_port` - Port for registering entity types
/// * `action_type_port` - Port for registering action types
/// * `schema_builder_port` - Port for building and persisting schemas
///
/// # Returns
///
/// An `Arc<dyn RegisterIamSchemaPort>` trait object, enabling dependency inversion
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::features::register_iam_schema::factories;
/// use hodei_policies::build_schema::factories;
/// use std::sync::Arc;
///
/// // Composition root creates the schema storage adapter
/// let schema_storage = Arc::new(SurrealSchemaStorage::new(db_client));
///
/// // Get hodei-policies ports via their factories
/// let (entity_port, action_port, schema_port) =
///     hodei_policies::build_schema::factories::create_schema_registration_components(schema_storage);
///
/// // Factory receives the ports and assembles the IAM use case
/// let iam_schema_uc = factories::create_register_iam_schema_use_case(
///     entity_port,
///     action_port,
///     schema_port,
/// );
///
/// let result = iam_schema_uc.register(command).await?;
/// ```
pub fn create_register_iam_schema_use_case(
    entity_type_port: Arc<dyn RegisterEntityTypePort>,
    action_type_port: Arc<dyn RegisterActionTypePort>,
    schema_builder_port: Arc<dyn BuildSchemaPort>,
) -> Arc<dyn RegisterIamSchemaPort> {
    debug!("Creating RegisterIamSchemaUseCase from ports");
    Arc::new(RegisterIamSchemaUseCase::new(
        entity_type_port,
        action_type_port,
        schema_builder_port,
    ))
}

/// Convenience factory that creates the complete IAM schema registration use case
/// from a storage adapter
///
/// This factory internally uses hodei-policies factories to create all required ports,
/// then assembles the IAM schema registration use case.
///
/// # Arguments
///
/// * `storage` - Pre-constructed implementation of SchemaStoragePort
///
/// # Returns
///
/// An `Arc<dyn RegisterIamSchemaPort>` trait object
///
/// # Example
///
/// ```rust,ignore
/// use hodei_iam::features::register_iam_schema::factories;
/// use std::sync::Arc;
///
/// // Composition root creates the adapter
/// let schema_storage = Arc::new(SurrealSchemaStorage::new(db_client));
///
/// // Factory handles all the wiring internally
/// let iam_schema_uc = factories::create_register_iam_schema_use_case_with_storage(schema_storage);
/// let result = iam_schema_uc.register(command).await?;
/// ```
pub fn create_register_iam_schema_use_case_with_storage<S: SchemaStoragePort + 'static>(
    storage: Arc<S>,
) -> Arc<dyn RegisterIamSchemaPort> {
    debug!("Creating RegisterIamSchemaUseCase with storage adapter");

    // Use hodei-policies factories to get the required ports
    let (entity_port, action_port, schema_port) =
        hodei_policies::build_schema::factories::create_schema_registration_components(storage);

    // Assemble and return the IAM schema registration use case
    create_register_iam_schema_use_case(entity_port, action_port, schema_port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use hodei_policies::build_schema::error::BuildSchemaError;

    /// Simple in-memory mock storage for schema persistence
    #[derive(Clone)]
    struct MockSchemaStorage {
        saved: std::sync::Arc<std::sync::Mutex<Vec<(String, Option<String>)>>>,
    }

    impl MockSchemaStorage {
        fn new() -> Self {
            Self {
                saved: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl SchemaStoragePort for MockSchemaStorage {
        async fn save_schema(
            &self,
            schema: String,
            version: Option<String>,
        ) -> Result<String, BuildSchemaError> {
            let mut lock = self.saved.lock().unwrap();
            lock.push((schema, version.clone()));
            Ok(format!("mock-schema-id-{}", lock.len()))
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
    fn test_factory_creates_use_case_with_storage() {
        let storage = Arc::new(MockSchemaStorage::new());

        let _uc = create_register_iam_schema_use_case_with_storage(storage);
        // Verify that the use case was created successfully
    }

    #[test]
    fn test_factory_creates_use_case_from_ports() {
        let storage = Arc::new(MockSchemaStorage::new());

        // Get ports from hodei-policies
        let (entity_port, action_port, schema_port) =
            hodei_policies::build_schema::factories::create_schema_registration_components(storage);

        let _uc = create_register_iam_schema_use_case(entity_port, action_port, schema_port);
        // Verify that the use case was created successfully
    }

    #[test]
    fn test_factory_returns_trait_object() {
        let storage = Arc::new(MockSchemaStorage::new());

        // Verify that the factory returns a trait object
        let uc: Arc<dyn RegisterIamSchemaPort> =
            create_register_iam_schema_use_case_with_storage(storage);

        // If this compiles, it proves we're returning the correct type
        assert!(Arc::strong_count(&uc) >= 1);
    }
}
