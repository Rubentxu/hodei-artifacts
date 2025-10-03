use cedar_policy::Policy;
use policies::features::list_policies::di::make_list_policies_use_case_mem;
use policies::features::list_policies::dto::ListPoliciesQuery;

#[tokio::test]
async fn test_list_policies_integration_empty() {
    // Arrange: Create use case with empty storage
    let (list_uc, _engine) = make_list_policies_use_case_mem()
        .await
        .expect("Failed to create list_policies use case");

    let query = ListPoliciesQuery::new();

    // Act: List policies
    let result = list_uc.execute(&query).await;

    // Assert: Should return empty list
    assert!(result.is_ok());
    let policies = result.unwrap();
    assert_eq!(policies.len(), 0);
}

#[tokio::test]
async fn test_list_policies_integration_with_data() {
    // Arrange: Create use case and add a policy
    let (list_uc, engine) = make_list_policies_use_case_mem()
        .await
        .expect("Failed to create list_policies use case");

    // Add a policy using the store
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    let query = ListPoliciesQuery::new();

    // Act: List policies
    let result = list_uc.execute(&query).await;

    // Assert: Should return at least 1 policy
    assert!(result.is_ok());
    let policies = result.unwrap();
    assert!(
        policies.len() >= 1,
        "Expected at least 1 policy, got {}",
        policies.len()
    );
}

#[tokio::test]
async fn test_list_policies_integration_after_create_and_delete() {
    // Arrange: Create use case and add a policy
    let (list_uc, engine) = make_list_policies_use_case_mem()
        .await
        .expect("Failed to create list_policies use case");

    // Add a policy
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    let policy_id = policy.id().to_string();
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    // Verify it's listed
    let query = ListPoliciesQuery::new();
    let policies = list_uc.execute(&query).await.expect("list policies");
    assert_eq!(policies.len(), 1);

    // Delete the policy
    engine
        .store
        .remove_policy(&policy_id)
        .await
        .expect("remove policy");

    // Act: List policies again
    let policies_after = list_uc
        .execute(&query)
        .await
        .expect("list policies after delete");

    // Assert: Should be empty
    assert_eq!(policies_after.len(), 0);
}
