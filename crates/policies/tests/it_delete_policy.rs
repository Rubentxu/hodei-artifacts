//! Integration tests for delete_policy feature
//!
//! This module contains integration tests for the delete_policy use case
//! using embedded in-memory SurrealDB for isolated testing.

#![cfg(feature = "integration")]

use chrono::Utc;
use policies::domain::ids::PolicyId;
use policies::domain::policy::{Policy, PolicyVersion};
use policies::features::delete_policy::adapter::{
    SimplePolicyDeletionAuditor,
    SurrealPolicyDeletionRetriever,
    SurrealPolicyDeletionStorage,
    SurrealPolicyDeletionValidator,
};
use policies::features::delete_policy::dto::DeletePolicyCommand;
use policies::features::delete_policy::ports::DeletionMode;
use policies::features::delete_policy::use_case::DeletePolicyUseCase;
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
        let policy_id = PolicyId::from_str("test-policy-delete").unwrap();
        let now = Utc::now();

        let policy = Policy {
            id: policy_id.clone(),
            name: "Policy to Delete".to_string(),
            description: Some("This policy will be deleted".to_string()),
            status: "active".to_string(),
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("hrn:hodei:iam:global:test-policy-delete/versions/1").unwrap(),
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

    #[tokio::test]
    async fn test_delete_policy_soft_deletion() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyDeletionValidator::new());
        let retriever = Arc::new(SurrealPolicyDeletionRetriever::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyDeletionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyDeletionAuditor::new());

        // Create use case
        let use_case = DeletePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        // Execute soft delete
        let command = DeletePolicyCommand {
            policy_id: policy_id.clone(),
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            deletion_mode: DeletionMode::Soft,
            reason: Some("Testing soft deletion".to_string()),
        };

        let result = use_case.execute(command).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.deletion_mode, DeletionMode::Soft);
        assert_eq!(response.message, "Policy soft deleted successfully");
        assert!(response.success);

        // Verify policy is marked as deleted but still exists
        let mut query_result = db_arc
            .query("SELECT * FROM policies WHERE id = $id")
            .bind(("id", policy_id.to_string()))
            .await
            .unwrap();

        let policies: Vec<Policy> = query_result.take(0).unwrap();
        assert_eq!(policies.len(), 1);
        let deleted_policy = &policies[0];
        assert_eq!(deleted_policy.status, "deleted");

        // Verify versions are still present
        let mut version_result = db_arc
            .query("SELECT * FROM policy_versions WHERE policy_id = $policy_id")
            .bind(("policy_id", policy_id.to_string()))
            .await
            .unwrap();

        let versions: Vec<PolicyVersion> = version_result.take(0).unwrap();
        assert_eq!(versions.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_policy_hard_deletion() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyDeletionValidator::new());
        let retriever = Arc::new(SurrealPolicyDeletionRetriever::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyDeletionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyDeletionAuditor::new());

        // Create use case
        let use_case = DeletePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        // Execute hard delete
        let command = DeletePolicyCommand {
            policy_id: policy_id.clone(),
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            deletion_mode: DeletionMode::Hard,
            reason: Some("Testing hard deletion".to_string()),
        };

        let result = use_case.execute(command).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.deletion_mode, DeletionMode::Hard);
        assert_eq!(response.message, "Policy permanently deleted");
        assert!(response.success);

        // Verify policy is completely removed
        let mut query_result = db_arc
            .query("SELECT * FROM policies WHERE id = $id")
            .bind(("id", policy_id.to_string()))
            .await
            .unwrap();

        let policies: Vec<Policy> = query_result.take(0).unwrap();
        assert_eq!(policies.len(), 0); // Policy should be gone

        // Verify versions are also removed
        let mut version_result = db_arc
            .query("SELECT * FROM policy_versions WHERE policy_id = $policy_id")
            .bind(("policy_id", policy_id.to_string()))
            .await
            .unwrap();

        let versions: Vec<PolicyVersion> = version_result.take(0).unwrap();
        assert_eq!(versions.len(), 0); // Versions should be gone
    }

    #[tokio::test]
    async fn test_delete_policy_archive_mode() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create initial policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let validator = Arc::new(SurrealPolicyDeletionValidator::new());
        let retriever = Arc::new(SurrealPolicyDeletionRetriever::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyDeletionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyDeletionAuditor::new());

        // Create use case
        let use_case = DeletePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        // Execute archive delete
        let command = DeletePolicyCommand {
            policy_id: policy_id.clone(),
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            deletion_mode: DeletionMode::Archive,
            reason: Some("Testing archive deletion".to_string()),
        };

        let result = use_case.execute(command).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.deletion_mode, DeletionMode::Archive);
        assert_eq!(response.message, "Policy archived successfully");
        assert!(response.success);

        // Verify policy is soft deleted but versions are archived
        let mut query_result = db_arc
            .query("SELECT * FROM policies WHERE id = $id")
            .bind(("id", policy_id.to_string()))
            .await
            .unwrap();

        let policies: Vec<Policy> = query_result.take(0).unwrap();
        assert_eq!(policies.len(), 1);
        let archived_policy = &policies[0];
        assert_eq!(archived_policy.status, "deleted");

        // Verify versions are still present (archived but not deleted)
        let mut version_result = db_arc
            .query("SELECT * FROM policy_versions WHERE policy_id = $policy_id")
            .bind(("policy_id", policy_id.to_string()))
            .await
            .unwrap();

        let versions: Vec<PolicyVersion> = version_result.take(0).unwrap();
        assert_eq!(versions.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_policy_not_found() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create adapters
        let validator = Arc::new(SurrealPolicyDeletionValidator::new());
        let retriever = Arc::new(SurrealPolicyDeletionRetriever::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyDeletionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyDeletionAuditor::new());

        // Create use case
        let use_case = DeletePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        // Try to delete non-existent policy
        let command = DeletePolicyCommand {
            policy_id: PolicyId::from_str("non-existent-policy").unwrap(),
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            deletion_mode: DeletionMode::Soft,
            reason: Some("Testing deletion of non-existent policy".to_string()),
        };

        let result = use_case.execute(command).await;

        // Should fail
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::delete_policy::error::DeletePolicyError::PolicyNotFound(_) => {},
            _ => panic!("Expected PolicyNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_delete_policy_system_policy_protection() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create a system policy
        let policy_id = PolicyId::from_str("system-policy").unwrap();
        let now = Utc::now();

        let system_policy = Policy {
            id: policy_id.clone(),
            name: "System Policy".to_string(),
            description: Some("Critical system policy".to_string()),
            status: "system".to_string(), // System status
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("hrn:system-policy/versions/1").unwrap(),
                policy_id: policy_id.clone(),
                version: 1,
                content: r#"permit(principal, action == "admin", resource);"#.to_string(),
                created_at: now,
                created_by: UserId::from_str("hrn:hodei:iam::system:user/system").unwrap(),
            },
        };

        // Insert system policy
        let _: Option<serde_json::Value> = db_arc
            .create(("policies", system_policy.id.to_string()))
            .content(&system_policy)
            .await?;

        // Create adapters
        let validator = Arc::new(SurrealPolicyDeletionValidator::new());
        let retriever = Arc::new(SurrealPolicyDeletionRetriever::new(db_arc.clone()));
        let storage = Arc::new(SurrealPolicyDeletionStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyDeletionAuditor::new());

        // Create use case
        let use_case = DeletePolicyUseCase::new(
            validator,
            retriever,
            storage,
            auditor,
        );

        // Try to delete system policy
        let command = DeletePolicyCommand {
            policy_id,
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            deletion_mode: DeletionMode::Soft,
            reason: Some("Trying to delete system policy".to_string()),
        };

        let result = use_case.execute(command).await;

        // Should fail due to system policy protection
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::delete_policy::error::DeletePolicyError::PolicyDeletionNotAllowed(_) => {},
            _ => panic!("Expected PolicyDeletionNotAllowed error"),
        }
    }
}
