//!
//! This module contains integration tests for the create_policy use case
//! using embedded in-memory SurrealDB for isolated testing.

#![cfg(feature = "integration")]

use std::sync::Arc;

use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

use policies::domain::ids::PolicyId;
use policies::domain::policy::{Policy, PolicyVersion};
use policies::features::create_policy::adapter::{
    SimplePolicyCreationAuditor,
    SurrealPolicyCreationStorage,
    SurrealPolicyCreationValidator,
    SurrealPolicyExistenceChecker,
};
use policies::features::create_policy::dto::CreatePolicyCommand;
use policies::features::create_policy::use_case::CreatePolicyUseCase;
use shared::hrn::{OrganizationId, UserId};

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    /// Setup embedded in-memory SurrealDB for testing
    async fn setup_memory_db() -> Result<Surreal<Any>, Box<dyn std::error::Error>> {
        let db = Surreal::new::<Any>("memory").await?;
        db.signin(Root {
            username: "root",
            password: "root",
        }).await?;
        db.use_ns("test").use_db("test").await?;

        // Create necessary tables
        db.query(r#"
            DEFINE TABLE policies SCHEMAFULL;
            DEFINE FIELD id ON policies TYPE string;
            DEFINE FIELD name ON policies TYPE string;
            DEFINE FIELD description ON policies TYPE option<string>;
            DEFINE FIELD status ON policies TYPE string DEFAULT "active";
            DEFINE FIELD version ON policies TYPE int DEFAULT 1;
            DEFINE FIELD created_at ON policies TYPE datetime;
            DEFINE FIELD updated_at ON policies TYPE datetime;
            DEFINE FIELD current_version ON policies TYPE object;

            DEFINE TABLE policy_versions SCHEMAFULL;
            DEFINE FIELD id ON policy_versions TYPE string;
            DEFINE FIELD policy_id ON policy_versions TYPE string;
            DEFINE FIELD version ON policy_versions TYPE int;
            DEFINE FIELD content ON policy_versions TYPE string;
            DEFINE FIELD created_at ON policy_versions TYPE datetime;
            DEFINE FIELD created_by ON policy_versions TYPE string;
        "#).await?;

        Ok(db)
    }

    /// Create test policy data
    fn create_test_command() -> CreatePolicyCommand {
        CreatePolicyCommand {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            name: "Test Policy".to_string(),
            description: Some("A test policy for integration testing".to_string()),
            organization_id: OrganizationId::from_str("hrn:hodei:iam::system:organization/test-org").unwrap(),
            content: r#"permit(principal, action == "read", resource);"#.to_string(),
            created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        }
    }

    #[tokio::test]
    async fn test_create_policy_integration_success() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create adapters
        let validator = Arc::new(SurrealPolicyCreationValidator::new());
        let existence_checker = Arc::new(SurrealPolicyExistenceChecker::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyCreationStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyCreationAuditor::new());

        // Create use case
        let use_case = CreatePolicyUseCase::new(
            validator,
            existence_checker,
            storage,
            auditor,
        );

        // Execute
        let command = create_test_command();
        let result = use_case.execute(command).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy_id.to_string(), "test-policy-123");
        assert_eq!(response.name, "Test Policy");
        assert_eq!(response.status, "active");
        assert_eq!(response.version, 1);

        // Verify policy was stored in database
        let mut query_result = db_arc
            .query("SELECT * FROM policies WHERE id = 'test-policy-123'")
            .await
            .unwrap();

        let policies: Vec<Policy> = query_result.take(0).unwrap();
        assert_eq!(policies.len(), 1);
        let stored_policy = &policies[0];
        assert_eq!(stored_policy.name, "Test Policy");
        assert_eq!(stored_policy.status, "active");
        assert_eq!(stored_policy.version, 1);

        // Verify version was stored
        let mut version_result = db_arc
            .query("SELECT * FROM policy_versions WHERE policy_id = 'test-policy-123'")
            .await
            .unwrap();

        let versions: Vec<PolicyVersion> = version_result.take(0).unwrap();
        assert_eq!(versions.len(), 1);
        let stored_version = &versions[0];
        assert_eq!(stored_version.version, 1);
        assert!(stored_version.content.contains("permit"));
    }

    #[tokio::test]
    async fn test_create_policy_duplicate_id() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create adapters
        let validator = Arc::new(SurrealPolicyCreationValidator::new());
        let existence_checker = Arc::new(SurrealPolicyExistenceChecker::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyCreationStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyCreationAuditor::new());

        // Create use case
        let use_case = CreatePolicyUseCase::new(
            validator.clone(),
            existence_checker.clone(),
            storage.clone(),
            auditor.clone(),
        );

        // Create first policy
        let command1 = create_test_command();
        let result1 = use_case.execute(command1).await;
        assert!(result1.is_ok());

        // Try to create policy with same ID
        let command2 = create_test_command();
        let result2 = use_case.execute(command2).await;

        // Should fail
        assert!(result2.is_err());
        let error = result2.unwrap_err();
        match error {
            policies::features::create_policy::error::CreatePolicyError::PolicyAlreadyExists(_) => {},
            _ => panic!("Expected PolicyAlreadyExists error"),
        }
    }

    #[tokio::test]
    async fn test_create_policy_invalid_syntax() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create adapters
        let validator = Arc::new(SurrealPolicyCreationValidator::new());
        let existence_checker = Arc::new(SurrealPolicyExistenceChecker::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyCreationStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyCreationAuditor::new());

        // Create use case
        let use_case = CreatePolicyUseCase::new(
            validator,
            existence_checker,
            storage,
            auditor,
        );

        // Create command with invalid Cedar syntax
        let mut command = create_test_command();
        command.content = "invalid cedar syntax {{{".to_string();

        // Execute
        let result = use_case.execute(command).await;

        // Should fail due to syntax error
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::create_policy::error::CreatePolicyError::PolicyValidationFailed(_) => {},
            _ => panic!("Expected PolicyValidationFailed error"),
        }
    }

    #[tokio::test]
    async fn test_create_policy_empty_content() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create adapters
        let validator = Arc::new(SurrealPolicyCreationValidator::new());
        let existence_checker = Arc::new(SurrealPolicyExistenceChecker::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyCreationStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyCreationAuditor::new());

        // Create use case
        let use_case = CreatePolicyUseCase::new(
            validator,
            existence_checker,
            storage,
            auditor,
        );

        // Create command with empty content
        let mut command = create_test_command();
        command.content = "".to_string();

        // Execute
        let result = use_case.execute(command).await;

        // Should fail due to empty content
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::create_policy::error::CreatePolicyError::PolicyValidationFailed(_) => {},
            _ => panic!("Expected PolicyValidationFailed error"),
        }
    }
}
