use cedar_policy::Policy;
use policies::features::delete_policy::di::make_use_case_mem;
use policies::features::delete_policy::dto::DeletePolicyCommand;
use policies::features::delete_policy::use_case::DeletePolicyError;

#[tokio::test]
async fn test_delete_policy_integration_success() {
    // Arrange: Create use case and add a policy
    let (delete_uc, engine) = make_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Add a policy
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    let policy_id = policy.id().to_string();
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    // Verify it exists
    let retrieved = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy");
    assert!(retrieved.is_some());

    // Act: Delete the policy
    let cmd = DeletePolicyCommand::new(policy_id.clone());
    let result = delete_uc.execute(&cmd).await;

    // Assert: Should succeed
    assert!(result.is_ok());
    assert!(result.unwrap());

    // Verify it's gone
    let retrieved_after = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy after delete");
    assert!(retrieved_after.is_none());
}

#[tokio::test]
async fn test_delete_policy_integration_not_found() {
    // Arrange: Create use case with empty storage
    let (delete_uc, _engine) = make_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Act: Try to delete non-existent policy
    let cmd = DeletePolicyCommand::new("nonexistent_policy_id");
    let result = delete_uc.execute(&cmd).await;

    // Assert: Should return NotFound error
    assert!(result.is_err());
    match result {
        Err(DeletePolicyError::NotFound(id)) => {
            assert_eq!(id, "nonexistent_policy_id");
        }
        _ => panic!("Expected NotFound error"),
    }
}

#[tokio::test]
async fn test_delete_policy_integration_invalid_id() {
    // Arrange: Create use case
    let (delete_uc, _engine) = make_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Act: Try to delete with empty ID
    let cmd = DeletePolicyCommand::new("");
    let result = delete_uc.execute(&cmd).await;

    // Assert: Should return InvalidCommand error
    assert!(result.is_err());
    match result {
        Err(DeletePolicyError::InvalidCommand(_)) => {}
        _ => panic!("Expected InvalidCommand error"),
    }
}

#[tokio::test]
async fn test_delete_policy_integration_idempotent() {
    // Arrange: Create use case and add a policy
    let (delete_uc, engine) = make_use_case_mem()
        .await
        .expect("Failed to create delete_policy use case");

    // Add a policy
    let policy_src = r#"permit(principal, action, resource);"#;
    let policy: Policy = policy_src.parse().expect("parse policy");
    let policy_id = policy.id().to_string();
    engine
        .store
        .add_policy(policy.clone())
        .await
        .expect("add policy");

    // Verify it exists
    let retrieved = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy");
    assert!(retrieved.is_some(), "Policy should exist before deletion");

    // Act: Delete the policy
    let cmd = DeletePolicyCommand::new(policy_id.clone());
    let result = delete_uc.execute(&cmd).await;
    assert!(result.is_ok(), "First deletion should succeed");

    // Assert: Policy is gone
    let retrieved_after = engine
        .store
        .get_policy(&policy_id)
        .await
        .expect("get policy after delete");
    assert!(retrieved_after.is_none(), "Policy should be deleted");

    // Act: Try to delete again
    let cmd2 = DeletePolicyCommand::new(policy_id.clone());
    let result2 = delete_uc.execute(&cmd2).await;

    // Assert: Should return NotFound error
    assert!(
        result2.is_err(),
        "Second deletion should fail with NotFound"
    );
    match result2 {
        Err(DeletePolicyError::NotFound(_)) => {}
        _ => panic!("Expected NotFound error on second deletion"),
    }
}
