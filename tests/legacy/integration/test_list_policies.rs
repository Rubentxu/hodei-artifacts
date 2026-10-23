//! Integration tests for list_policies handler

use crate::common::{TestDb, insert_test_policy, setup_test_db, valid_policy_content};
use hodei_iam::features::list_policies::use_case::ListPoliciesUseCase;
use hodei_iam::infrastructure::surreal::policy_adapter::SurrealPolicyAdapter;
use kernel::HodeiPolicy;
use std::sync::Arc;
use tracing_test::traced_test;

#[tokio::test]
#[traced_test]
async fn test_list_policies_empty() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = ListPoliciesUseCase::new(adapter);

    let query = hodei_iam::features::list_policies::dto::ListPoliciesQuery {
        limit: 10,
        offset: 0,
    };

    let result = use_case.execute(query).await;
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 0);
    assert_eq!(response.total_count, 0);
    assert!(!response.has_next_page);
    assert!(!response.has_previous_page);
}

#[tokio::test]
#[traced_test]
async fn test_list_policies_first_page() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = ListPoliciesUseCase::new(adapter);

    // Insert 25 policies
    for i in 0..25 {
        let policy = HodeiPolicy::new(format!("policy-{}", i), valid_policy_content());
        insert_test_policy(&db.client, policy).await.unwrap();
    }

    let query = hodei_iam::features::list_policies::dto::ListPoliciesQuery {
        limit: 10,
        offset: 0,
    };

    let result = use_case.execute(query).await;
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 10);
    assert_eq!(response.total_count, 25);
    assert!(response.has_next_page);
    assert!(!response.has_previous_page);
}

#[tokio::test]
#[traced_test]
async fn test_list_policies_middle_page() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = ListPoliciesUseCase::new(adapter);

    // Insert 25 policies
    for i in 0..25 {
        let policy = HodeiPolicy::new(format!("policy-{}", i), valid_policy_content());
        insert_test_policy(&db.client, policy).await.unwrap();
    }

    let query = hodei_iam::features::list_policies::dto::ListPoliciesQuery {
        limit: 10,
        offset: 10,
    };

    let result = use_case.execute(query).await;
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 10);
    assert!(response.has_next_page);
    assert!(response.has_previous_page);
}

#[tokio::test]
#[traced_test]
async fn test_list_policies_last_page() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = ListPoliciesUseCase::new(adapter);

    // Insert 25 policies
    for i in 0..25 {
        let policy = HodeiPolicy::new(format!("policy-{}", i), valid_policy_content());
        insert_test_policy(&db.client, policy).await.unwrap();
    }

    let query = hodei_iam::features::list_policies::dto::ListPoliciesQuery {
        limit: 10,
        offset: 20,
    };

    let result = use_case.execute(query).await;
    assert!(result.is_ok());
    let response = result.unwrap();
    assert_eq!(response.policies.len(), 5);
    assert!(!response.has_next_page);
    assert!(response.has_previous_page);
}

#[tokio::test]
#[traced_test]
async fn test_list_policies_invalid_limit_zero() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = ListPoliciesUseCase::new(adapter);

    let query = hodei_iam::features::list_policies::dto::ListPoliciesQuery {
        limit: 0,
        offset: 0,
    };

    let result = use_case.execute(query).await;
    assert!(result.is_err());
}

#[tokio::test]
#[traced_test]
async fn test_list_policies_invalid_limit_over_100() {
    let db = setup_test_db().await;
    let adapter = Arc::new(SurrealPolicyAdapter::new(Arc::new(db.client.clone())));
    let use_case = ListPoliciesUseCase::new(adapter);

    let query = hodei_iam::features::list_policies::dto::ListPoliciesQuery {
        limit: 101,
        offset: 0,
    };

    let result = use_case.execute(query).await;
    assert!(result.is_err());
}
