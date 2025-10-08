//! Integration test for Update Policy feature (HU-IAM-007)
//!
//! This test validates the complete vertical slice for updating IAM policies,
//! using ONLY the public API of the hodei-iam crate without accessing any
//! internal modules.
//!
//! # Test Coverage
//!
//! - Update policy content only
//! - Update description only
//! - Update both content and description
//! - Error handling (policy not found, invalid content, no updates)
//! - Validation through the public API
//!
//! # Acceptance Criteria (HU-IAM-007)
//!
//! 1. ✅ El nuevo contenido de la política debe ser validado sintácticamente
//! 2. ✅ La operación debe ser atómica
//! 3. ✅ Si la política no existe, se debe devolver un error PolicyNotFound
//! 4. ✅ [Test de Integración] Debe existir un test que use el UpdatePolicyUseCase
//!    público para modificar una política y verifique que el cambio se ha persistido

use hodei_iam::features::create_policy::CedarPolicyValidator;
use hodei_iam::features::create_policy::{CreatePolicyCommand, CreatePolicyUseCase};
use hodei_iam::features::list_policies::{ListPoliciesQuery, ListPoliciesUseCase};
use hodei_iam::features::update_policy::{
    UpdatePolicyCommand, UpdatePolicyError, UpdatePolicyUseCase,
};
use hodei_iam::infrastructure::surreal::SurrealPolicyAdapter;
use std::sync::Arc;
use surrealdb::{Surreal, engine::local::Mem};

/// Helper to create a use case with SurrealDB adapter pre-populated with test data
async fn setup_use_case_with_policy() -> (
    UpdatePolicyUseCase<CedarPolicyValidator, SurrealPolicyAdapter>,
    Arc<SurrealPolicyAdapter>,
) {
    let db = Arc::new(Surreal::new::<Mem>(()).await.unwrap());
    db.use_ns("test").use_db("iam").await.unwrap();
    let validator = Arc::new(CedarPolicyValidator::new());
    let adapter = Arc::new(SurrealPolicyAdapter::new(db));

    // Create a policy first using the create policy use case
    let create_use_case = hodei_iam::features::create_policy::CreatePolicyUseCase::new(
        adapter.clone(),
        validator.clone(),
    );
    let create_command = hodei_iam::features::create_policy::CreatePolicyCommand {
        policy_id: "test-policy".to_string(),
        policy_content: "permit(principal, action, resource);".to_string(),
        description: Some("Original description".to_string()),
    };

    let _ = create_use_case.execute(create_command).await.unwrap();

    let use_case = UpdatePolicyUseCase::new(validator, adapter.clone());
    (use_case, adapter)
}

#[tokio::test]
async fn test_update_policy_content_through_public_api() {
    // Arrange
    let (use_case, adapter) = setup_use_case_with_policy().await;

    let new_content = r#"
        permit(
            principal,
            action == Action::"ReadDocument",
            resource
        );
    "#;

    // Act - Using ONLY public API
    let command = UpdatePolicyCommand::update_content("test-policy", new_content);
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok(), "Expected successful update");
    let updated_policy = result.unwrap();

    assert_eq!(updated_policy.name, "test-policy");
    assert!(updated_policy.content.contains("ReadDocument"));
    assert_eq!(
        updated_policy.description,
        Some("Original description".to_string())
    );

    // Verify persistence by listing policies
    let list_use_case = hodei_iam::features::list_policies::ListPoliciesUseCase::new(adapter);
    let list_query = hodei_iam::features::list_policies::ListPoliciesQuery::default();
    let list_result = list_use_case.execute(list_query).await.unwrap();
    assert_eq!(list_result.policies.len(), 1);
    let listed_policy = &list_result.policies[0];
    assert!(
        listed_policy
            .description
            .as_ref()
            .unwrap()
            .contains("Original description")
    );
}

#[tokio::test]
async fn test_update_policy_description_only_through_public_api() {
    // Arrange
    let (use_case, adapter) = setup_use_case_with_policy().await;

    // Act - Update only description
    let command = UpdatePolicyCommand::update_description(
        "test-policy",
        "Updated description with more details",
    );
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok(), "Expected successful update");
    let updated_policy = result.unwrap();

    assert_eq!(updated_policy.name, "test-policy");
    assert_eq!(
        updated_policy.description,
        Some("Updated description with more details".to_string())
    );
    // Content should remain unchanged
    assert_eq!(
        updated_policy.content,
        "permit(principal, action, resource);"
    );

    // Verify persistence by listing policies
    let list_use_case = hodei_iam::features::list_policies::ListPoliciesUseCase::new(adapter);
    let list_query = hodei_iam::features::list_policies::ListPoliciesQuery::default();
    let list_result = list_use_case.execute(list_query).await.unwrap();
    assert_eq!(list_result.policies.len(), 1);
    let listed_policy = &list_result.policies[0];
    assert_eq!(
        listed_policy.description,
        Some("Updated description with more details".to_string())
    );
}

#[tokio::test]
async fn test_update_policy_both_content_and_description() {
    // Arrange
    let (use_case, adapter) = setup_use_case_with_policy().await;

    let new_content = "forbid(principal, action, resource);";
    let new_description = "Policy now forbids all actions";

    // Act - Update both fields
    let command = UpdatePolicyCommand::update_both("test-policy", new_content, new_description);
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok(), "Expected successful update");
    let updated_policy = result.unwrap();

    assert!(updated_policy.content.contains("forbid"));
    assert_eq!(
        updated_policy.description,
        Some("Policy now forbids all actions".to_string())
    );

    // Verify persistence
    let (content, desc) = adapter.get_policy("test-policy").unwrap();
    assert_eq!(content, new_content);
    assert_eq!(desc, Some(new_description.to_string()));
}

#[tokio::test]
async fn test_update_nonexistent_policy_returns_not_found() {
    // Arrange
    let (use_case, _adapter) = setup_use_case_with_policy().await;

    // Act - Try to update a policy that doesn't exist
    let command = UpdatePolicyCommand::update_description("nonexistent-policy", "New description");
    let result = use_case.execute(command).await;

    // Assert - Should return PolicyNotFound error
    assert!(result.is_err(), "Expected PolicyNotFound error");
    match result.unwrap_err() {
        UpdatePolicyError::PolicyNotFound(id) => {
            assert_eq!(id, "nonexistent-policy");
        }
        other => panic!("Expected PolicyNotFound, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_update_with_invalid_cedar_syntax_fails() {
    // Arrange
    let (use_case, _adapter) = setup_use_case_with_policy().await;

    // Act - Try to update with invalid Cedar syntax
    let invalid_content = "this is not valid cedar syntax at all!!!";
    let command = UpdatePolicyCommand::update_content("test-policy", invalid_content);
    let result = use_case.execute(command).await;

    // Assert - Should return InvalidPolicyContent error
    assert!(result.is_err(), "Expected InvalidPolicyContent error");
    match result.unwrap_err() {
        UpdatePolicyError::InvalidPolicyContent(_msg) => {
            // Expected error
        }
        other => panic!("Expected InvalidPolicyContent, got: {:?}", other),
    }

    // Verify original policy remains unchanged (atomicity)
    let (new_use_case, _) = setup_use_case_with_policy().await;
    let list_use_case = hodei_iam::features::list_policies::ListPoliciesUseCase::new(adapter);
    let list_query = hodei_iam::features::list_policies::ListPoliciesQuery::default();
    let list_result = list_use_case.execute(list_query).await.unwrap();
    assert_eq!(list_result.policies.len(), 1);
    let listed_policy = &list_result.policies[0];
    assert_eq!(
        listed_policy.description,
        Some("Original description".to_string())
    );
}

#[tokio::test]
async fn test_update_with_empty_content_fails() {
    // Arrange
    let (use_case, _adapter) = setup_use_case_with_policy().await;

    // Act - Try to update with empty content
    let command = UpdatePolicyCommand::update_content("test-policy", "   ");
    let result = use_case.execute(command).await;

    // Assert - Should return EmptyPolicyContent error
    assert!(result.is_err(), "Expected EmptyPolicyContent error");
    assert!(matches!(
        result.unwrap_err(),
        UpdatePolicyError::EmptyPolicyContent
    ));
}

#[tokio::test]
async fn test_update_with_no_changes_fails() {
    // Arrange
    let (use_case, _adapter) = setup_use_case_with_policy().await;

    // Act - Try to update without providing any fields
    let command = UpdatePolicyCommand {
        policy_id: "test-policy".to_string(),
        policy_content: None,
        description: None,
    };
    let result = use_case.execute(command).await;

    // Assert - Should return NoUpdatesProvided error
    assert!(result.is_err(), "Expected NoUpdatesProvided error");
    assert!(matches!(
        result.unwrap_err(),
        UpdatePolicyError::NoUpdatesProvided
    ));
}

#[tokio::test]
async fn test_update_policy_with_empty_id_fails() {
    // Arrange
    let (use_case, _adapter) = setup_use_case_with_policy().await;

    // Act - Try to update with empty policy ID
    let command = UpdatePolicyCommand::update_description("", "New description");
    let result = use_case.execute(command).await;

    // Assert - Should return InvalidPolicyId error
    assert!(result.is_err(), "Expected InvalidPolicyId error");
    match result.unwrap_err() {
        UpdatePolicyError::InvalidPolicyId(_msg) => {
            // Expected error
        }
        other => panic!("Expected InvalidPolicyId, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_multiple_sequential_updates_preserve_state() {
    // Arrange
    let (use_case, adapter) = setup_use_case_with_policy().await;

    // Act - Perform multiple updates sequentially

    // Update 1: Change description
    let cmd1 = UpdatePolicyCommand::update_description("test-policy", "First update");
    use_case.execute(cmd1).await.unwrap();

    // Update 2: Change content
    let cmd2 =
        UpdatePolicyCommand::update_content("test-policy", "forbid(principal, action, resource);");
    use_case.execute(cmd2).await.unwrap();

    // Update 3: Change description again
    let cmd3 = UpdatePolicyCommand::update_description("test-policy", "Final description");
    let result = use_case.execute(cmd3).await.unwrap();

    // Assert - Final state should have both last content and last description
    assert_eq!(result.content, "forbid(principal, action, resource);");
    assert_eq!(result.description, Some("Final description".to_string()));

    // Verify persistence by listing policies
    let list_use_case = ListPoliciesUseCase::new(adapter);
    let list_query = ListPoliciesQuery::default();
    let list_result = list_use_case.execute(list_query).await.unwrap();
    assert_eq!(list_result.policies.len(), 1);
    let listed_policy = &list_result.policies[0];
    assert_eq!(
        listed_policy.description,
        Some("Policy now forbids all actions".to_string())
    );
}

#[tokio::test]
async fn test_update_policy_validates_complex_cedar_policy() {
    // Arrange
    let (use_case, adapter) = setup_use_case_with_policy().await;

    // Act - Update with a complex but valid Cedar policy
    let complex_policy = r#"
        permit(
            principal in Group::"Admins",
            action in [Action::"Read", Action::"Write"],
            resource
        ) when {
            resource.owner == principal.id &&
            context.ip_address.isIpv4()
        };
    "#;

    let command = UpdatePolicyCommand::update_content("test-policy", complex_policy);
    let result = use_case.execute(command).await;

    // Assert
    assert!(result.is_ok(), "Complex policy should be valid");
    let updated = result.unwrap();
    assert!(updated.content.contains("Admins"));
    assert!(updated.content.contains("isIpv4"));

    // Verify persistence by listing policies
    let list_use_case = ListPoliciesUseCase::new(adapter);
    let list_query = ListPoliciesQuery::default();
    let list_result = list_use_case.execute(list_query).await.unwrap();
    assert_eq!(list_result.policies.len(), 1);
    let listed_policy = &list_result.policies[0];
    assert_eq!(
        listed_policy.description,
        Some("Final description".to_string())
    );
}

#[tokio::test]
async fn test_update_preserves_unchanged_fields() {
    // Arrange
    let db = Arc::new(Surreal::new::<Mem>(()).await.unwrap());
    db.use_ns("test").use_db("iam").await.unwrap();
    let validator = Arc::new(CedarPolicyValidator::new());
    let adapter = Arc::new(SurrealPolicyAdapter::new(db));

    // Create policy with both content and description using create policy use case
    let create_use_case = CreatePolicyUseCase::new(adapter.clone(), validator.clone());
    let create_command = CreatePolicyCommand {
        policy_id: "preserve-test".to_string(),
        policy_content: "permit(principal, action, resource);".to_string(),
        description: Some("Original description".to_string()),
    };

    let _ = create_use_case.execute(create_command).await.unwrap();

    let use_case = UpdatePolicyUseCase::new(validator, adapter.clone());

    // Act - Update only content, description should be preserved
    let command = UpdatePolicyCommand::update_content(
        "preserve-test",
        "forbid(principal, action, resource);",
    );
    let result = use_case.execute(command).await.unwrap();

    // Assert
    assert_eq!(result.content, "forbid(principal, action, resource);");
    assert_eq!(result.description, Some("Original description".to_string()));

    // Now update only description, content should be preserved
    let command2 = UpdatePolicyCommand::update_description("preserve-test", "New description");
    let result2 = use_case.execute(command2).await.unwrap();

    assert_eq!(result2.content, "forbid(principal, action, resource);");
    assert_eq!(result2.description, Some("New description".to_string()));
}
