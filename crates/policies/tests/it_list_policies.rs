//! Integration tests for list_policies feature
//!
//! This module contains integration tests for the list_policies use case
//! using embedded in-memory SurrealDB for isolated testing.

#![cfg(feature = "integration")]

use chrono::Utc;
use policies::domain::ids::PolicyId;
use policies::domain::policy::{Policy, PolicyVersion};
use policies::features::list_policies::adapter::{
    SimplePolicyListingAuditor,
    SurrealListQueryValidator,
    SurrealPolicyListingStorage,
};
use policies::features::list_policies::dto::ListPoliciesQuery;
use policies::features::list_policies::ports::ListPoliciesConfig;
use policies::features::list_policies::use_case::ListPoliciesUseCase;
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

    /// Create multiple test policies in the database
    async fn create_test_policies(db: &Surreal<Any>) -> Result<Vec<PolicyId>, Box<dyn std::error::Error>> {
        let mut policy_ids = Vec::new();
        let now = Utc::now();

        // Create policy 1
        let policy_id1 = PolicyId::from_str("test-policy-list-1").unwrap();
        let policy1 = Policy {
            id: policy_id1.clone(),
            name: "Security Policy".to_string(),
            description: Some("Policy for security controls".to_string()),
            status: "active".to_string(),
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("test-policy-list-1/versions/1").unwrap(),
                policy_id: policy_id1.clone(),
                version: 1,
                content: r#"permit(principal, action == "read", resource);"#.to_string(),
                created_at: now,
                created_by: UserId::from_str("hrn:hodei:iam::system:user/admin").unwrap(),
            },
        };

        let _: Option<serde_json::Value> = db
            .create(("policies", policy1.id.to_string()))
            .content(&policy1)
            .await?;
        policy_ids.push(policy_id1);

        // Create policy 2
        let policy_id2 = PolicyId::from_str("test-policy-list-2").unwrap();
        let policy2 = Policy {
            id: policy_id2.clone(),
            name: "Access Policy".to_string(),
            description: Some("Policy for access management".to_string()),
            status: "draft".to_string(),
            version: 2,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("test-policy-list-2/versions/2").unwrap(),
                policy_id: policy_id2.clone(),
                version: 2,
                content: r#"permit(principal, action == "write", resource);"#.to_string(),
                created_at: now,
                created_by: UserId::from_str("hrn:hodei:iam::system:user/admin").unwrap(),
            },
        };

        let _: Option<serde_json::Value> = db
            .create(("policies", policy2.id.to_string()))
            .content(&policy2)
            .await?;
        policy_ids.push(policy_id2);

        // Create policy 3
        let policy_id3 = PolicyId::from_str("test-policy-list-3").unwrap();
        let policy3 = Policy {
            id: policy_id3.clone(),
            name: "Audit Policy".to_string(),
            description: Some("Policy for audit controls".to_string()),
            status: "active".to_string(),
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("test-policy-list-3/versions/1").unwrap(),
                policy_id: policy_id3.clone(),
                version: 1,
                content: r#"permit(principal, action == "audit", resource);"#.to_string(),
                created_at: now,
                created_by: UserId::from_str("hrn:hodei:iam::system:user/auditor").unwrap(),
            },
        };

        let _: Option<serde_json::Value> = db
            .create(("policies", policy3.id.to_string()))
            .content(&policy3)
            .await?;
        policy_ids.push(policy_id3);

        Ok(policy_ids)
    }

    #[tokio::test]
    async fn test_list_policies_all() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create test policies
        let _policy_ids = create_test_policies(&db_arc).await.unwrap();

        // Create adapters
        let config = ListPoliciesConfig::default();
        let query_validator = Arc::new(SurrealListQueryValidator::new(
            config.max_limit,
            config.max_offset,
        ));
        let storage = Arc::new(SurrealPolicyListingStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyListingAuditor::new());

        // Create use case
        let use_case = ListPoliciesUseCase::new(
            query_validator,
            storage,
            auditor,
            config,
        );

        // Execute list all policies
        let query = ListPoliciesQuery {
            organization_id: None,
            name_filter: None,
            status_filter: None,
            created_by_filter: None,
            limit: Some(10),
            offset: Some(0),
            sort_by: Some("name".to_string()),
            sort_order: Some("asc".to_string()),
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 3);
        assert_eq!(response.total_count, 3);
        assert!(!response.has_more);

        // Check ordering (should be alphabetical by name)
        assert_eq!(response.policies[0].name, "Access Policy");
        assert_eq!(response.policies[1].name, "Audit Policy");
        assert_eq!(response.policies[2].name, "Security Policy");
    }

    #[tokio::test]
    async fn test_list_policies_with_filter() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create test policies
        let _policy_ids = create_test_policies(&db_arc).await.unwrap();

        // Create adapters
        let config = ListPoliciesConfig::default();
        let query_validator = Arc::new(SurrealListQueryValidator::new(
            config.max_limit,
            config.max_offset,
        ));
        let storage = Arc::new(SurrealPolicyListingStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyListingAuditor::new());

        // Create use case
        let use_case = ListPoliciesUseCase::new(
            query_validator,
            storage,
            auditor,
            config,
        );

        // Execute list with status filter
        let query = ListPoliciesQuery {
            organization_id: None,
            name_filter: None,
            status_filter: Some("active".to_string()),
            created_by_filter: None,
            limit: Some(10),
            offset: Some(0),
            sort_by: Some("name".to_string()),
            sort_order: Some("asc".to_string()),
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 2); // Only active policies
        assert_eq!(response.total_count, 2);
        assert!(!response.has_more);

        // Check that both returned policies are active
        for policy in &response.policies {
            assert_eq!(policy.status, "active");
        }
    }

    #[tokio::test]
    async fn test_list_policies_with_name_filter() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create test policies
        let _policy_ids = create_test_policies(&db_arc).await.unwrap();

        // Create adapters
        let config = ListPoliciesConfig::default();
        let query_validator = Arc::new(SurrealListQueryValidator::new(
            config.max_limit,
            config.max_offset,
        ));
        let storage = Arc::new(SurrealPolicyListingStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyListingAuditor::new());

        // Create use case
        let use_case = ListPoliciesUseCase::new(
            query_validator,
            storage,
            auditor,
            config,
        );

        // Execute list with name filter
        let query = ListPoliciesQuery {
            organization_id: None,
            name_filter: Some("Security".to_string()),
            status_filter: None,
            created_by_filter: None,
            limit: Some(10),
            offset: Some(0),
            sort_by: Some("name".to_string()),
            sort_order: Some("asc".to_string()),
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 1);
        assert_eq!(response.total_count, 1);
        assert_eq!(response.policies[0].name, "Security Policy");
    }

    #[tokio::test]
    async fn test_list_policies_pagination() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create test policies
        let _policy_ids = create_test_policies(&db_arc).await.unwrap();

        // Create adapters
        let config = ListPoliciesConfig::default();
        let query_validator = Arc::new(SurrealListQueryValidator::new(
            config.max_limit,
            config.max_offset,
        ));
        let storage = Arc::new(SurrealPolicyListingStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyListingAuditor::new());

        // Create use case
        let use_case = ListPoliciesUseCase::new(
            query_validator,
            storage,
            auditor,
            config,
        );

        // Execute list with pagination (limit 2)
        let query = ListPoliciesQuery {
            organization_id: None,
            name_filter: None,
            status_filter: None,
            created_by_filter: None,
            limit: Some(2),
            offset: Some(0),
            sort_by: Some("name".to_string()),
            sort_order: Some("asc".to_string()),
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 2);
        assert_eq!(response.total_count, 3);
        assert!(response.has_more); // Because total_count > returned count
    }

    #[tokio::test]
    async fn test_list_policies_empty_result() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Don't create any policies

        // Create adapters
        let config = ListPoliciesConfig::default();
        let query_validator = Arc::new(SurrealListQueryValidator::new(
            config.max_limit,
            config.max_offset,
        ));
        let storage = Arc::new(SurrealPolicyListingStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyListingAuditor::new());

        // Create use case
        let use_case = ListPoliciesUseCase::new(
            query_validator,
            storage,
            auditor,
            config,
        );

        // Execute list
        let query = ListPoliciesQuery {
            organization_id: None,
            name_filter: None,
            status_filter: None,
            created_by_filter: None,
            limit: Some(10),
            offset: Some(0),
            sort_by: Some("name".to_string()),
            sort_order: Some("asc".to_string()),
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Verify
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 0);
        assert_eq!(response.total_count, 0);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_list_policies_invalid_limit() {
        // Setup
        let db = setup_memory_db().await.unwrap();
        let db_arc = Arc::new(db);

        // Create test policies
        let _policy_ids = create_test_policies(&db_arc).await.unwrap();

        // Create adapters
        let config = ListPoliciesConfig::default();
        let query_validator = Arc::new(SurrealListQueryValidator::new(
            config.max_limit,
            config.max_offset,
        ));
        let storage = Arc::new(SurrealPolicyListingStorage::new(db_arc.clone()));
        let auditor = Arc::new(SimplePolicyListingAuditor::new());

        // Create use case
        let use_case = ListPoliciesUseCase::new(
            query_validator,
            storage,
            auditor,
            config,
        );

        // Execute list with invalid limit (too high)
        let query = ListPoliciesQuery {
            organization_id: None,
            name_filter: None,
            status_filter: None,
            created_by_filter: None,
            limit: Some(200), // Higher than max_limit (100)
            offset: Some(0),
            sort_by: Some("name".to_string()),
            sort_order: Some("asc".to_string()),
        };

        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        // Should fail due to limit exceeded
        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            policies::features::list_policies::error::ListPoliciesError::QueryLimitExceeded { max, requested } => {
                assert_eq!(max, 100);
                assert_eq!(requested, 200);
            },
            _ => panic!("Expected QueryLimitExceeded error"),
        }
    }
}
