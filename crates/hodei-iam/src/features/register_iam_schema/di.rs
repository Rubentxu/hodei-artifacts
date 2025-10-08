//! Dependency Injection (DI) / Factory helpers for the `register_iam_schema` feature.
//!
//! This module centralizes construction logic so the composition root (e.g. an Axum
//! application or a CLI) can assemble the vertical slice without knowing internal
//! details about the wiring of policy engine use cases.
//!
//! Architectural Goals
//! -------------------
//! * Keep all wiring in one place
//! * Enable swapping storage implementations (different `SchemaStoragePort` adapters)
//! * Provide explicit factories for readability & testability
//! * Avoid leaking concrete adapter types to higher layers (only use Arcs + traits)
//! * Do NOT expose internal types like `EngineBuilder` from `hodei-policies`

use std::sync::Arc;
use tracing::{debug, instrument};

use hodei_policies::{
    build_schema::{self, BuildSchemaUseCase},
    register_action_type::RegisterActionTypeUseCase,
    register_entity_type::RegisterEntityTypeUseCase,
};

use crate::features::register_iam_schema::use_case::RegisterIamSchemaUseCase;

/// Factory for creating RegisterIamSchemaUseCase instances with proper dependency injection
pub struct RegisterIamSchemaUseCaseFactory;

impl RegisterIamSchemaUseCaseFactory {
    /// Build the feature use case from its three dependent use cases.
    ///
    /// This is the primary constructor that accepts already-constructed dependencies.
    ///
    /// # Arguments
    ///
    /// * `entity_uc` - Use case for registering entity types
    /// * `action_uc` - Use case for registering action types
    /// * `schema_uc` - Use case for building and persisting schemas
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::Arc;
    /// use hodei_policies::build_schema;
    /// use hodei_iam::features::register_iam_schema::RegisterIamSchemaUseCaseFactory;
    ///
    /// let storage = Arc::new(MySchemaStorage::new());
    ///
    /// // Use the public bundle factory to get all components
    /// let (entity_uc, action_uc, schema_uc) =
    ///     build_schema::di::create_schema_registration_components(storage);
    ///
    /// let uc = RegisterIamSchemaUseCaseFactory::from_use_cases(entity_uc, action_uc, schema_uc);
    /// ```
    #[instrument(level = "debug", skip(entity_uc, action_uc, schema_uc))]
    pub fn from_use_cases<S>(
        entity_uc: Arc<RegisterEntityTypeUseCase>,
        action_uc: Arc<RegisterActionTypeUseCase>,
        schema_uc: BuildSchemaUseCase<S>,
    ) -> RegisterIamSchemaUseCase
    where
        S: build_schema::ports::SchemaStoragePort + 'static,
    {
        debug!("Assembling RegisterIamSchemaUseCase from provided use cases");
        RegisterIamSchemaUseCase::new(entity_uc, action_uc, schema_uc)
    }

    /// Recommended factory that creates the use case with only a storage adapter.
    ///
    /// This factory uses the public API from `hodei-policies` without exposing
    /// internal types like `EngineBuilder`. All wiring is handled internally.
    ///
    /// # Arguments
    ///
    /// * `storage` - Schema storage adapter implementation
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use std::sync::Arc;
    /// use hodei_iam::features::register_iam_schema::RegisterIamSchemaUseCaseFactory;
    ///
    /// let storage = Arc::new(MySchemaStorage::new());
    /// let uc = RegisterIamSchemaUseCaseFactory::build_with_storage(storage);
    /// ```
    #[instrument(level = "debug", skip(storage))]
    pub fn build_with_storage<S>(storage: Arc<S>) -> RegisterIamSchemaUseCase
    where
        S: build_schema::ports::SchemaStoragePort + 'static,
    {
        debug!("Creating RegisterIamSchemaUseCase using public bundle factory");

        // Use the public bundle factory from hodei-policies
        let (entity_uc, action_uc, schema_uc) =
            build_schema::di::create_schema_registration_components(storage);

        RegisterIamSchemaUseCase::new(entity_uc, action_uc, schema_uc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use build_schema::ports::SchemaStoragePort;

    /// Simple in-memory mock storage for schema persistence
    struct MockSchemaStorage {
        saved: std::sync::Mutex<Vec<(String, Option<String>)>>,
    }

    impl MockSchemaStorage {
        fn new() -> Self {
            Self {
                saved: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl SchemaStoragePort for MockSchemaStorage {
        async fn save_schema(
            &self,
            schema: String,
            version: Option<String>,
        ) -> Result<String, build_schema::error::BuildSchemaError> {
            let mut lock = self.saved.lock().unwrap();
            lock.push((schema, version.clone()));
            Ok(format!("mock-schema-id-{}", lock.len()))
        }

        async fn get_latest_schema(
            &self,
        ) -> Result<Option<String>, build_schema::error::BuildSchemaError> {
            Ok(None)
        }

        async fn get_schema_by_version(
            &self,
            _version: &str,
        ) -> Result<Option<String>, build_schema::error::BuildSchemaError> {
            Ok(None)
        }

        async fn delete_schema(
            &self,
            _schema_id: &str,
        ) -> Result<bool, build_schema::error::BuildSchemaError> {
            Ok(false)
        }

        async fn list_schema_versions(
            &self,
        ) -> Result<Vec<String>, build_schema::error::BuildSchemaError> {
            Ok(vec![])
        }
    }

    #[test]
    fn factory_builds_use_case_with_storage() {
        let storage = Arc::new(MockSchemaStorage::new());

        let _uc = RegisterIamSchemaUseCaseFactory::build_with_storage(storage);
        // Use case is successfully constructed without exposing EngineBuilder
    }

    #[test]
    fn factory_builds_use_case_from_existing_use_cases() {
        let storage = Arc::new(MockSchemaStorage::new());

        // Use public bundle factory to get components
        let (entity_uc, action_uc, schema_uc) =
            build_schema::di::create_schema_registration_components(storage);

        let _uc = RegisterIamSchemaUseCaseFactory::from_use_cases(entity_uc, action_uc, schema_uc);
        // Use case is successfully constructed from pre-built components
    }
}
