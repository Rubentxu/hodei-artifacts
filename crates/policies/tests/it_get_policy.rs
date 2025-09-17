//! Integration tests for get_policy feature
//!
//! This module contains integration tests for the get_policy use case
//! using embedded in-memory SurrealDB for isolated testing.

#![cfg(feature = "integration")]

use chrono::Utc;
use policies::domain::ids::PolicyId;
use policies::domain::policy::{Policy, PolicyVersion};
use policies::features::get_policy::adapter::{
    SimplePolicyRetrievalAuditor,
    SurrealPolicyAccessValidator,
    SurrealPolicyRetrievalStorage,
};
use policies::features::get_policy::dto::GetPolicyQuery;
use policies::features::get_policy::use_case::GetPolicyUseCase;
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
        let policy_id = PolicyId::from_str("test-policy-get").unwrap();
        let now = Utc::now();

        let policy = Policy {
            id: policy_id.clone(),
            name: "Test Get Policy".to_string(),
            description: Some("A policy for get testing".to_string()),
            status: "active".to_string(),
            version: 2,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("test-policy-get/versions/2").unwrap(),
                policy_id: policy_id.clone(),
                version: 2,
                content: r#"permit(principal, action == "read", resource)
                when { principal.clearance >= resource.classification };"#.to_string(),
                created_at: now,
                created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            },
        };

        // Insert policy
        let _: Option<serde_json::Value> = db
            .create(("policies", policy.id.to_string()))
            .content(&policy)
            .await?;

        // Insert current version
        let _: Option<serde_json::Value> = db
            .create(("policy_versions", policy.current_version.id.to_string()))
            .content(&policy.current_version)
            .await?;

        // Insert an older version
        let old_version = PolicyVersion {
            id: Hrn::new("test-policy-get/versions/1").unwrap(),
            policy_id: policy_id.clone(),
            version: 1,
            content: r#"permit(principal, action == "read", resource);"#.to_string(),
            created_at: now - chrono::Duration::hours(1),
            created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        };

        let _: Option<serde_json::Value> = db
            .create(("policy_versions", old_version.id.to_string()))
            .content(&old_version)
            .await?;

        Ok(policy_id)
    }

    #[tokio::test]
    async fn test_get_policy_success() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create test policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let access_validator = Arc::new(SurrealPolicyAccessValidator::new());
        let storage = Arc::new(SurrealPolicyRetrievalStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyRetrievalAuditor::new());

        // Create use case
        let use_case = GetPolicyUseCase::new(
            access_validator,
            storage,
            auditor,
        );

        // Execute get policy
        let query = GetPolicyQuery {
            policy_id: policy_id.clone(),
            include_versions: false,
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy_id.to_string(), policy_id.to_string());
        assert_eq!(response.name, "Test Get Policy");
        assert_eq!(response.status, "active");
        assert_eq!(response.version, 2);
        assert!(response.content.contains("principal.clearance"));
    }

    #[tokio::test]
    async fn test_get_policy_not_found() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create adapters
        let access_validator = Arc::new(SurrealPolicyAccessValidator::new());
        let storage = Arc::new(SurrealPolicyRetrievalStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyRetrievalAuditor::new());

        // Create use case
        let use_case = GetPolicyUseCase::new(
            access_validator,
            storage,
            auditor,
        );

        // Try to get non-existent policy
        let query = GetPolicyQuery {
            policy_id: PolicyId::from_str("non-existent-policy").unwrap(),
            include_versions: false,
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Should fail
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::get_policy::error::GetPolicyError::PolicyNotFound(_) => {},
            _ => panic!("Expected PolicyNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_get_policy_with_versions() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create test policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let access_validator = Arc::new(SurrealPolicyAccessValidator::new());
        let storage = Arc::new(SurrealPolicyRetrievalStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyRetrievalAuditor::new());

        // Create use case
        let use_case = GetPolicyUseCase::new(
            access_validator,
            storage,
            auditor,
        );

        // Execute get policy with versions
        let query = GetPolicyQuery {
            policy_id: policy_id.clone(),
            include_versions: true,
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy_id.to_string(), policy_id.to_string());
        assert_eq!(response.version, 2);
    }

    #[tokio::test]
    async fn test_get_policy_detailed_info() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create test policy
        let policy_id = create_test_policy(&db_arc).await.unwrap();

        // Create adapters
        let access_validator = Arc::new(SurrealPolicyAccessValidator::new());
        let storage = Arc::new(SurrealPolicyRetrievalStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyRetrievalAuditor::new());

        // Create use case
        let use_case = GetPolicyUseCase::new(
            access_validator,
            storage,
            auditor,
        );

        // Execute get policy details
        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.get_policy_details(&policy_id, &user_id).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy_id.to_string(), policy_id.to_string());
        assert_eq!(response.name, "Test Get Policy");
        assert_eq!(response.version, 2);
        assert_eq!(response.versions_count, 1); // Only counting versions in current implementation
        assert!(response.content.contains("principal.clearance"));
    }

    #[tokio::test]
    async fn test_get_policy_deleted_policy() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create a deleted policy
        let policy_id = PolicyId::from_str("deleted-policy").unwrap();
        let now = Utc::now();

        let deleted_policy = Policy {
            id: policy_id.clone(),
            name: "Deleted Policy".to_string(),
            description: Some("This policy was deleted".to_string()),
            status: "deleted".to_string(), // Deleted status
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("deleted-policy/versions/1").unwrap(),
                policy_id: policy_id.clone(),
                version: 1,
                content: r#"permit(principal, action == "read", resource);"#.to_string(),
                created_at: now,
                created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            },
        };

        // Insert deleted policy
        let _: Option<serde_json::Value> = db_arc
            .create(("policies", deleted_policy.id.to_string()))
            .content(&deleted_policy)
            .await?;

        // Create adapters
        let access_validator = Arc::new(SurrealPolicyAccessValidator::new());
        let storage = Arc::new(SurrealPolicyRetrievalStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyRetrievalAuditor::new());

        // Create use case
        let use_case = GetPolicyUseCase::new(
            access_validator,
            storage,
            auditor,
        );

        // Try to get deleted policy
        let query = GetPolicyQuery {
            policy_id: policy_id.clone(),
            include_versions: false,
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Should fail because deleted policies are not returned
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::get_policy::error::GetPolicyError::PolicyNotFound(_) => {},
            _ => panic!("Expected PolicyNotFound error for deleted policy"),
        }
    }
}
