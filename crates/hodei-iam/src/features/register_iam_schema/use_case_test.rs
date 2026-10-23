//! Unit tests for RegisterIamSchemaUseCase
//!
//! These tests verify the business logic for registering the IAM schema,
//! including entity type registration, action type registration, and schema building.
//!
//! # Why there are two kinds of test here
//!
//! The mocked tests assert the *orchestration*: which types are registered, in
//! which order, and how failures propagate. They cannot assert that anything
//! was really registered into a Cedar schema.
//!
//! The last test in this file closes that gap: it wires the **real**
//! `hodei-policies` registrars against an in-memory storage and asserts the
//! schema that comes out. That is the test that fails when the registration
//! path silently does nothing — the defect the mocked tests used to hide
//! behind hardcoded counts.

use crate::features::register_iam_schema::{
    dto::RegisterIamSchemaCommand,
    error::RegisterIamSchemaError,
    mocks::{
        create_default_mocks, create_failing_mocks, create_mocks_with_failing_action,
        create_mocks_with_failing_build, create_mocks_with_failing_entity,
        create_mocks_with_version_and_id,
    },
    use_case::RegisterIamSchemaUseCase,
};

/// The entity types the IAM schema is expected to declare.
const EXPECTED_ENTITY_TYPES: [&str; 2] = ["Iam::Group", "Iam::User"];

/// The action types the IAM schema is expected to declare.
const EXPECTED_ACTION_TYPES: [&str; 6] = [
    "AddUserToGroup",
    "CreateGroup",
    "CreateUser",
    "DeleteGroup",
    "DeleteUser",
    "RemoveUserFromGroup",
];

/// Test that the IAM schema can be registered successfully with valid input
#[tokio::test]
async fn test_register_iam_schema_success() {
    // Setup
    let (entity_mock, action_mock, build_mock) = create_default_mocks();

    let use_case =
        RegisterIamSchemaUseCase::new(entity_mock.clone(), action_mock.clone(), build_mock.clone());

    // Execute
    let cmd = RegisterIamSchemaCommand::new();
    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let result = result.unwrap();

    // Should register 2 entity types (User, Group)
    assert_eq!(result.entity_types_registered, 2);

    // Should register 6 action types (CreateUser, DeleteUser, CreateGroup, DeleteGroup, AddUserToGroup, RemoveUserFromGroup)
    assert_eq!(result.action_types_registered, 6);

    // Should have default schema version
    assert_eq!(result.schema_version, "latest");
    assert_eq!(result.schema_id, "test-schema-id");
    assert!(result.validated);
}

/// Test that the use case asks the registrars for the domain types themselves
///
/// The counts alone would also be satisfied by a hardcoded tally, so the
/// names of the registered types are asserted too.
#[tokio::test]
async fn test_register_iam_schema_registers_the_domain_types() {
    let (entity_mock, action_mock, build_mock) = create_default_mocks();

    let use_case =
        RegisterIamSchemaUseCase::new(entity_mock.clone(), action_mock.clone(), build_mock.clone());

    use_case
        .execute(RegisterIamSchemaCommand::new())
        .await
        .expect("the registration must succeed");

    let mut entity_types = entity_mock.registered_entity_types();
    entity_types.sort();
    assert_eq!(entity_types, EXPECTED_ENTITY_TYPES);

    let mut action_types = action_mock.registered_action_types();
    action_types.sort();
    assert_eq!(action_types, EXPECTED_ACTION_TYPES);
}

/// Test that schema registration fails when entity registration fails
#[tokio::test]
async fn test_register_iam_schema_entity_registration_error() {
    // Setup - only entity mock fails
    let (entity_mock, action_mock, build_mock) = create_mocks_with_failing_entity();

    let use_case = RegisterIamSchemaUseCase::new(entity_mock, action_mock, build_mock);

    // Execute
    let cmd = RegisterIamSchemaCommand::new();
    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RegisterIamSchemaError::EntityTypeRegistrationError(_) => {} // Expected
        _ => panic!("Expected EntityTypeRegistrationError"),
    }
}

/// Test that schema registration fails when action registration fails
#[tokio::test]
async fn test_register_iam_schema_action_registration_error() {
    // Setup - only action mock fails
    let (entity_mock, action_mock, build_mock) = create_mocks_with_failing_action();

    let use_case = RegisterIamSchemaUseCase::new(entity_mock, action_mock, build_mock);

    // Execute
    let cmd = RegisterIamSchemaCommand::new();
    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RegisterIamSchemaError::ActionTypeRegistrationError(_) => {} // Expected
        _ => panic!("Expected ActionTypeRegistrationError"),
    }
}

/// Test that schema registration fails when schema building fails
#[tokio::test]
async fn test_register_iam_schema_build_error() {
    // Setup - only build mock fails
    let (entity_mock, action_mock, build_mock) = create_mocks_with_failing_build();

    let use_case = RegisterIamSchemaUseCase::new(entity_mock, action_mock, build_mock);

    // Execute
    let cmd = RegisterIamSchemaCommand::new();
    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_err());
    match result.unwrap_err() {
        RegisterIamSchemaError::SchemaBuildError(_) => {} // Expected
        _ => panic!("Expected SchemaBuildError"),
    }
}

/// Test that schema registration works with specific version
#[tokio::test]
async fn test_register_iam_schema_with_version() {
    // Setup
    let (entity_mock, action_mock, build_mock) =
        create_mocks_with_version_and_id("v1.0.0".to_string(), "schema-v1".to_string());

    let use_case = RegisterIamSchemaUseCase::new(entity_mock, action_mock, build_mock);

    // Execute
    let cmd = RegisterIamSchemaCommand::new().with_version("v1.0.0".to_string());
    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let result = result.unwrap();
    assert_eq!(result.schema_version, "v1.0.0");
    assert_eq!(result.schema_id, "schema-v1");
    assert_eq!(result.entity_types_registered, 2);
    assert_eq!(result.action_types_registered, 6);
}

/// Test that schema registration works without validation
#[tokio::test]
async fn test_register_iam_schema_without_validation() {
    // Setup
    let (entity_mock, action_mock, build_mock) = create_default_mocks();

    let use_case = RegisterIamSchemaUseCase::new(entity_mock, action_mock, build_mock);

    // Execute
    let cmd = RegisterIamSchemaCommand::new().with_validation(false);
    let result = use_case.execute(cmd).await;

    // Assert
    assert!(result.is_ok());
    let result = result.unwrap();
    assert_eq!(result.entity_types_registered, 2);
    assert_eq!(result.action_types_registered, 6);
    assert_eq!(result.schema_version, "latest");
}

/// Test that all mocks failing results in first error (entity registration)
#[tokio::test]
async fn test_register_iam_schema_all_mocks_failing() {
    // Setup - all mocks fail
    let (entity_mock, action_mock, build_mock) = create_failing_mocks();

    let use_case = RegisterIamSchemaUseCase::new(entity_mock, action_mock, build_mock);

    // Execute
    let cmd = RegisterIamSchemaCommand::new();
    let result = use_case.execute(cmd).await;

    // Assert - should fail on first error (entity registration)
    assert!(result.is_err());
    match result.unwrap_err() {
        RegisterIamSchemaError::EntityTypeRegistrationError(_) => {} // Expected
        _ => panic!("Expected EntityTypeRegistrationError"),
    }
}

// ---------------------------------------------------------------------------
// The real components
// ---------------------------------------------------------------------------

/// The schemas the in-memory storage was asked to persist, with the version
/// they were stored under.
type SavedSchemas = std::sync::Arc<std::sync::Mutex<Vec<(String, Option<String>)>>>;

/// An in-memory schema storage that keeps what it was given
struct InMemorySchemaStorage {
    saved: SavedSchemas,
}

impl InMemorySchemaStorage {
    fn new() -> Self {
        Self {
            saved: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn saved_count(&self) -> usize {
        self.saved.lock().map(|saved| saved.len()).unwrap_or(0)
    }
}

#[async_trait::async_trait]
impl hodei_policies::build_schema::ports::SchemaStoragePort for InMemorySchemaStorage {
    async fn save_schema(
        &self,
        schema: String,
        version: Option<String>,
    ) -> Result<String, hodei_policies::build_schema::error::BuildSchemaError> {
        let mut lock = self.saved.lock().map_err(|e| {
            hodei_policies::build_schema::error::BuildSchemaError::SchemaStorageError(e.to_string())
        })?;
        lock.push((schema, version.clone()));
        Ok(format!("in-memory-schema-{}", lock.len()))
    }

    async fn get_latest_schema(
        &self,
    ) -> Result<Option<String>, hodei_policies::build_schema::error::BuildSchemaError> {
        Ok(None)
    }

    async fn get_schema_by_version(
        &self,
        _version: &str,
    ) -> Result<Option<String>, hodei_policies::build_schema::error::BuildSchemaError> {
        Ok(None)
    }

    async fn delete_schema(
        &self,
        _schema_id: &str,
    ) -> Result<bool, hodei_policies::build_schema::error::BuildSchemaError> {
        Ok(false)
    }

    async fn list_schema_versions(
        &self,
    ) -> Result<Vec<String>, hodei_policies::build_schema::error::BuildSchemaError> {
        Ok(vec![])
    }
}

/// The registration really reaches a Cedar schema
///
/// This is the test that the mocked tests above cannot replace: it wires the
/// real `hodei-policies` registrars, so a registration that does nothing
/// produces an empty schema (and the use case fails) instead of a green test
/// with hardcoded counts.
#[tokio::test]
async fn test_register_iam_schema_builds_a_real_cedar_schema() {
    let storage = std::sync::Arc::new(InMemorySchemaStorage::new());

    let (entity_port, action_port, schema_port) =
        hodei_policies::build_schema::factories::create_schema_registration_components(
            storage.clone(),
        );

    let use_case = RegisterIamSchemaUseCase::new(entity_port, action_port, schema_port);

    let result = use_case
        .execute(RegisterIamSchemaCommand::new().with_validation(true))
        .await
        .expect("the IAM schema must be registrable with the real components");

    assert_eq!(
        result.entity_types_registered, 2,
        "the built schema must declare the two IAM entity types"
    );
    assert_eq!(
        result.action_types_registered, 6,
        "the built schema must declare the six IAM action types"
    );
    assert!(
        result.validated,
        "the built schema must survive Cedar's own validation"
    );
    assert_eq!(
        storage.saved_count(),
        1,
        "the built schema must be persisted exactly once"
    );
}
