//! Integration tests for update_policy feature
//!
//! This module contains integration tests for the update_policy use case
//! using embedded in-memory SurrealDB for isolated testing.

#![cfg(feature = "integration")]

use chrono::Utc;
use policies::domain::ids::PolicyId;
use policies::domain::policy::{Policy, PolicyVersion};
use policies::features::update_policy::adapter::{
    SimplePolicyUpdateAuditor,
    SurrealPolicyRetriever,
    SurrealPolicyUpdateStorage,
    SurrealPolicyUpdateValidator,
};
use policies::features::update_policy::dto::UpdatePolicyCommand;
use policies::features::update_policy::use_case::UpdatePolicyUseCase;
use shared::hrn::Hrn;
use shared::hrn::UserId;
use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

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

    /// Create a test policy in the database
    async fn create_test_policy(db: &Surreal<Any>) -> Result<PolicyId, Box<dyn std::error::Error>> {
        let policy_id = PolicyId::from_str("test-policy-update").unwrap();
        let now = Utc::now();

        let policy = Policy {
            id: policy_id.clone(),
            name: "Original Policy".to_string(),
            description: Some("Original description".to_string()),
            status: "active".to_string(),
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("test-policy-update/versions/1").unwrap(),
                policy_id: policy_id.clone(),
                version: 1,
                content: r#"permit(principal, action == "read", resource);"#.to_string(),
                created_at: now,
                created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            },
        };

        // Insert policy
        let _: Option<serde_json::Value> = db
            .create(("policies", policy.id.to_string()))
            .content(&policy)
            .await?;

        // Insert version
        let _: Option<serde_json::Value> = db
            .create(("policy_versions", policy.current_version.id.to_string()))
            .content(&policy.current_version)
            .await?;

        Ok(policy_id)
    }

    /// Create test update command
    fn create_update_command(policy_id: PolicyId) -> UpdatePolicyCommand {
        UpdatePolicyCommand {
            name: Some("Updated Policy Name".to_string()),
            description: Some("Updated description".to_string()),
            content: Some(r#"permit(principal, action == "write", resource)
            when { principal.clearance >= resource.classification };"#.to_string()),
            expected_version: Some(1),
            updated_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        }
    }

    #[tokio::test]
    async fn test_update_policy_integration_success() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyUpdateValidator::new());
        let retriever = Arc::new(SurrealPolicyRetriever::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyUpdateStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyUpdateAuditor::new());

        // Create use case
        let use_case = UpdatePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        // Execute update
        let command = create_update_command(policy_id.clone());
        let result = use_case.execute(&policy_id, command).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.name, "Updated Policy Name");
        assert_eq!(response.description, Some("Updated description".to_string()));
        assert_eq!(response.version, 2); // Version should be incremented

        // Verify policy was updated in database
        let mut query_result = db_arc
            .query("SELECT * FROM policies WHERE id = $id")
            .bind(("id", policy_id.to_string()))
            .await
            .unwrap();

        let policies: Vec<Policy> = query_result.take(0).unwrap();
        assert_eq!(policies.len(), 1);
        let updated_policy = &policies[0];
        assert_eq!(updated_policy.name, "Updated Policy Name");
        assert_eq!(updated_policy.version, 2);

        // Verify new version was created
        let mut version_result = db_arc
            .query("SELECT * FROM policy_versions WHERE policy_id = $policy_id ORDER BY version DESC")
            .bind(("policy_id", policy_id.to_string()))
            .await
            .unwrap();

        let versions: Vec<PolicyVersion> = version_result.take(0).unwrap();
        assert_eq!(versions.len(), 2); // Original + new version
        assert_eq!(versions[0].version, 2); // Latest version first
        assert!(versions[0].content.contains("write"));
    }

    #[tokio::test]
    async fn test_update_policy_version_conflict() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyUpdateValidator::new());
        let retriever = Arc::new(SurrealPolicyRetriever::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyUpdateStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyUpdateAuditor::new());

        // Create use case
        let use_case = UpdatePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        // Create update command with wrong expected version
        let mut command = create_update_command(policy_id.clone());
        command.expected_version = Some(2); // Policy is at version 1

        // Execute update
        let result = use_case.execute(&policy_id, command).await;

        // Should fail due to version conflict
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::update_policy::error::UpdatePolicyError::PolicyVersionConflict { expected, actual } => {
                assert_eq!(expected, 2);
                assert_eq!(actual, 1);
            },
            _ => panic!("Expected PolicyVersionConflict error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_partial_update() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyUpdateValidator::new());
        let retriever = Arc::new(SurrealPolicyRetriever::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyUpdateStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyUpdateAuditor::new());

        // Create use case
        let use_case = UpdatePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        // Update only name (partial update)
        let command = UpdatePolicyCommand {
            name: Some("Only Name Updated".to_string()),
            description: None,
            content: None,
            expected_version: None,
            updated_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        };

        // Execute update
        let result = use_case.execute(&policy_id, command).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.name, "Only Name Updated");
        assert_eq!(response.version, 1); // Version should not change for partial updates

        // Verify in database
        let mut query_result = db_arc
            .query("SELECT * FROM policies WHERE id = $id")
            .bind(("id", policy_id.to_string()))
            .await
            .unwrap();

        let policies: Vec<Policy> = query_result.take(0).unwrap();
        let updated_policy = &policies[0];
        assert_eq!(updated_policy.name, "Only Name Updated");
        assert_eq!(updated_policy.version, 1); // No version increment
    }

    #[tokio::test]
    async fn test_update_policy_invalid_syntax() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyUpdateValidator::new());
        let retriever = Arc::new(SurrealPolicyRetriever::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyUpdateStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyUpdateAuditor::new());

        // Create use case
        let use_case = UpdatePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        // Update with invalid Cedar syntax
        let command = UpdatePolicyCommand {
            name: None,
            description: None,
            content: Some("invalid {{{ syntax".to_string()),
            expected_version: None,
            updated_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        };

        // Execute update
        let result = use_case.execute(&policy_id, command).await;

        // Should fail due to syntax error
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::update_policy::error::UpdatePolicyError::PolicyValidationFailed(_) => {},
            _ => panic!("Expected PolicyValidationFailed error"),
        }
    }
}
