//! Integration tests for manage_policy_versions feature
//!
//! This module contains integration tests for the manage_policy_versions use case
//! using embedded in-memory SurrealDB for isolated testing.

#![cfg(feature = "integration")]

use chrono::Utc;
use policies::domain::ids::PolicyId;
use policies::domain::policy::{Policy, PolicyVersion};
use policies::features::manage_policy_versions::adapter::{
    SimplePolicyVersionAuditor,
    SurrealPolicyVersionHistory,
    SurrealPolicyVersionStorage,
    SurrealPolicyVersionValidator,
};
use policies::features::manage_policy_versions::dto::{CreatePolicyVersionCommand, GetPolicyVersionsQuery, RollbackPolicyVersionCommand};
use policies::features::manage_policy_versions::use_case::ManagePolicyVersionsUseCase;
use shared::hrn::Hrn;
use shared::hrn::UserId;
use std::sync::Arc;
use surrealdb::engine::any::Any;
use surrealdb::opt::auth::Root;
use surrealdb::Surreal;

#[cfg(test)]
mod tests {
    use super::*;

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
        let policy_id = PolicyId::from("test-policy-versions");
        let now = Utc::now();

        let policy = Policy {
            id: policy_id.clone(),
            name: "Version Test Policy".to_string(),
            description: Some("A policy for version testing".to_string()),
            status: "active".to_string(),
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("policy", "test-policy-versions/versions/1").unwrap(),
                policy_id: policy_id.clone(),
                version: 1,
                content: r#"permit(principal, action == "read", resource);"#.to_string(),
                created_at: now,
                created_by: UserId::from("test-user"),
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

    #[tokio::test]
    async fn test_create_version_integration_success() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyVersionValidator::new());
        let history = Arc::new(SurrealPolicyVersionHistory::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyVersionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyVersionAuditor::new());

        // Create use case
        let use_case = ManagePolicyVersionsUseCase::new(
            validator,
            history,
            storage,
            auditor,
        );

        // Execute create version
        let command = CreatePolicyVersionCommand {
            policy_id: policy_id.clone(),
            content: r#"permit(principal, action == "write", resource)
            when { principal.clearance >= resource.classification };"#.to_string(),
            created_by: UserId::from("test-user"),
        };

        let result = use_case.create_version(command).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy_id.to_string(), policy_id.to_string());
        assert_eq!(response.version, 2);
        assert!(response.content.contains("write"));

        // Verify version was stored in database
        let mut query_result = db_arc
            .query("SELECT * FROM policy_versions WHERE policy_id = $policy_id AND version = 2")
            .bind(("policy_id", policy_id.to_string()))
            .await
            .unwrap();

        let versions: Vec<PolicyVersion> = query_result.take(0).unwrap();
        assert_eq!(versions.len(), 1);
        let stored_version = &versions[0];
        assert_eq!(stored_version.version, 2);
        assert!(stored_version.content.contains("write"));
    }

    #[tokio::test]
    async fn test_get_versions_integration() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy with multiple versions
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Add a second version manually
        let version2 = PolicyVersion {
            id: Hrn::new("policy", "test-policy-versions/versions/2").unwrap(),
            policy_id: policy_id.clone(),
            version: 2,
            content: r#"permit(principal, action == "write", resource);"#.to_string(),
            created_at: Utc::now(),
            created_by: UserId::from("test-user"),
        };

        let _: Option<serde_json::Value> = db_arc
            .create(("policy_versions", version2.id.to_string()))
            .content(&version2)
            .await?;

        // Create adapters
        let validator = Arc::new(SurrealPolicyVersionValidator::new());
        let history = Arc::new(SurrealPolicyVersionHistory::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyVersionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyVersionAuditor::new());

        // Create use case
        let use_case = ManagePolicyVersionsUseCase::new(
            validator,
            history,
            storage,
            auditor,
        );

        // Execute get versions
        let query = GetPolicyVersionsQuery {
            policy_id: policy_id.clone(),
            limit: None,
            offset: None,
        };

        let result = use_case.get_versions(query).await;

        // Verify
        assert!(result.is_ok());
        let versions = result.unwrap();
        assert_eq!(versions.len(), 2);

        // Check versions are ordered by version number descending
        assert_eq!(versions[0].version, 2);
        assert_eq!(versions[1].version, 1);
    }

    #[tokio::test]
    async fn test_rollback_version_integration() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Add version 2
        let version2 = PolicyVersion {
            id: Hrn::new("policy", "test-policy-versions/versions/2").unwrap(),
            policy_id: policy_id.clone(),
            version: 2,
            content: r#"permit(principal, action == "write", resource);"#.to_string(),
            created_at: Utc::now(),
            created_by: UserId::from("test-user"),
        };

        let _: Option<serde_json::Value> = db_arc
            .create(("policy_versions", version2.id.to_string()))
            .content(&version2)
            .await?;

        // Update policy to version 2
        let _: Option<serde_json::Value> = db_arc
            .query("UPDATE policies SET version = 2, current_version = $version WHERE id = $id")
            .bind(("id", policy_id.to_string()))
            .bind(("version", serde_json::to_value(&version2).unwrap()))
            .await
            .unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyVersionValidator::new());
        let history = Arc::new(SurrealPolicyVersionHistory::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyVersionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyVersionAuditor::new());

        // Create use case
        let use_case = ManagePolicyVersionsUseCase::new(
            validator,
            history,
            storage,
            auditor,
        );

        // Execute rollback to version 1
        let command = RollbackPolicyVersionCommand {
            policy_id: policy_id.clone(),
            target_version: 1,
            rollback_by: UserId::from("test-user"),
            reason: Some("Testing rollback functionality".to_string()),
        };

        let result = use_case.rollback_to_version(command).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.from_version, 2);
        assert_eq!(response.to_version, 1);
        assert!(response.success);

        // Verify policy was rolled back in database
        let mut query_result = db_arc
            .query("SELECT * FROM policies WHERE id = $id")
            .bind(("id", policy_id.to_string()))
            .await
            .unwrap();

        let policies: Vec<Policy> = query_result.take(0).unwrap();
        assert_eq!(policies.len(), 1);
        let updated_policy = &policies[0];
        assert_eq!(updated_policy.version, 1);
        assert!(updated_policy.current_version.content.contains("read"));
    }

    #[tokio::test]
    async fn test_create_version_invalid_content() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyVersionValidator::new());
        let history = Arc::new(SurrealPolicyVersionHistory::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyVersionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyVersionAuditor::new());

        // Create use case
        let use_case = ManagePolicyVersionsUseCase::new(
            validator,
            history,
            storage,
            auditor,
        );

        // Execute create version with empty content
        let command = CreatePolicyVersionCommand {
            policy_id: policy_id.clone(),
            content: "".to_string(),
            created_by: UserId::from("test-user"),
        };

        let result = use_case.create_version(command).await;

        // Should fail due to empty content
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::manage_policy_versions::error::ManagePolicyVersionsError::VersionHistoryError(_) => {},
            _ => panic!("Expected VersionHistoryError"),
        }
    }

    #[tokio::test]
    async fn test_get_version_history_empty() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyVersionValidator::new());
        let history = Arc::new(SurrealPolicyVersionHistory::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyVersionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyVersionAuditor::new());

        // Create use case
        let use_case = ManagePolicyVersionsUseCase::new(
            validator,
            history,
            storage,
            auditor,
        );

        // Execute get version history
        let result = use_case.get_version_history(&policy_id, Some(5)).await;

        // Verify
        assert!(result.is_ok());
        let versions = result.unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, 1);
    }

    #[tokio::test]
    async fn test_compare_versions_integration() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Add version 2
        let version2 = PolicyVersion {
            id: Hrn::new("policy", "test-policy-versions/versions/2").unwrap(),
            policy_id: policy_id.clone(),
            version: 2,
            content: r#"permit(principal, action == "write", resource);"#.to_string(),
            created_at: Utc::now(),
            created_by: UserId::from("test-user"),
        };

        let _: Option<serde_json::Value> = db_arc
            .create(("policy_versions", version2.id.to_string()))
            .content(&version2)
            .await?;

        // Create adapters
        let validator = Arc::new(SurrealPolicyVersionValidator::new());
        let history = Arc::new(SurrealPolicyVersionHistory::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyVersionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyVersionAuditor::new());

        // Create use case
        let use_case = ManagePolicyVersionsUseCase::new(
            validator,
            history,
            storage,
            auditor,
        );

        // Execute compare versions
        let result = use_case.compare_versions(&policy_id, 1, 2).await;

        // Verify
        assert!(result.is_ok());
        let diff = result.unwrap();
        assert!(diff.contains("Version differences"));
        assert!(diff.contains("From (v1)"));
        assert!(diff.contains("To (v2)"));
    }
}
