//! Unit tests for IAM policy management use cases
//!
//! These tests verify the behavior of policy CRUD operations with mocked dependencies.

#[cfg(test)]
mod tests {
    use crate::features::create_policy::dto::{
        CreatePolicyCommand, DeletePolicyCommand, GetPolicyQuery, ListPoliciesQuery,
        UpdatePolicyCommand,
    };
    use crate::features::create_policy::mocks::{MockPolicyPersister, MockPolicyValidator};
    use crate::features::create_policy::use_case::{
        CreatePolicyUseCase, DeletePolicyUseCase, GetPolicyUseCase, ListPoliciesUseCase,
        UpdatePolicyUseCase,
    };
    use crate::internal::domain::{Hrn, Policy};
    use std::collections::HashMap;
    use std::sync::Arc;

    // ============================================================================
    // CreatePolicyUseCase Tests
    // ============================================================================

    #[tokio::test]
    async fn test_create_policy_success() {
        let persister = Arc::new(MockPolicyPersister::new());
        let validator = Arc::new(MockPolicyValidator::new_always_valid());
        let use_case = CreatePolicyUseCase::new(persister, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test policy".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());

        let policy_dto = result.unwrap();
        assert_eq!(policy_dto.id.to_string(), "hrn:iam:policy:test-policy");
        assert_eq!(policy_dto.content, "permit(principal, action, resource);");
        assert_eq!(policy_dto.description, Some("Test policy".to_string()));
    }

    #[tokio::test]
    async fn test_create_policy_invalid_content() {
        let persister = Arc::new(MockPolicyPersister::new());
        let validator = Arc::new(MockPolicyValidator::new_always_invalid(vec![
            "Parse error: invalid syntax".to_string(),
        ]));
        let use_case = CreatePolicyUseCase::new(persister, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "invalid policy content".to_string(),
            description: Some("Test policy".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        match error {
            crate::features::create_policy::error::CreatePolicyError::InvalidPolicyContent(msg) => {
                assert!(msg.contains("Parse error"));
            }
            _ => panic!("Expected InvalidPolicyContent error, got {:?}", error),
        }
    }

    #[tokio::test]
    async fn test_create_policy_validation_failed() {
        let persister = Arc::new(MockPolicyPersister::new());
        let validator = Arc::new(MockPolicyValidator::new_always_invalid(vec![
            "Type mismatch in condition".to_string(),
        ]));
        let use_case = CreatePolicyUseCase::new(persister, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource) when { false };".to_string(),
            description: Some("Test policy".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_policy_already_exists() {
        let mut policies = HashMap::new();
        let existing_policy = Policy {
            id: Hrn::new("iam", "policy", "existing-policy"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing policy".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        policies.insert("existing-policy".to_string(), existing_policy);

        let persister = Arc::new(MockPolicyPersister::with_policies(policies));
        let validator = Arc::new(MockPolicyValidator::new_always_valid());
        let use_case = CreatePolicyUseCase::new(persister, validator);

        let command = CreatePolicyCommand {
            policy_id: "existing-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test policy".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        match error {
            crate::features::create_policy::error::CreatePolicyError::PolicyAlreadyExists => {}
            _ => panic!("Expected PolicyAlreadyExists error, got {:?}", error),
        }
    }

    #[tokio::test]
    async fn test_create_policy_with_multiple_validation_errors() {
        let persister = Arc::new(MockPolicyPersister::new());
        let validator = Arc::new(MockPolicyValidator::new_always_invalid(vec![
            "Error 1: Missing principal".to_string(),
            "Error 2: Invalid action".to_string(),
        ]));
        let use_case = CreatePolicyUseCase::new(persister, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "invalid content".to_string(),
            description: Some("Test policy".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        match error {
            crate::features::create_policy::error::CreatePolicyError::InvalidPolicyContent(msg) => {
                assert!(msg.contains("Error 1"));
                assert!(msg.contains("Error 2"));
            }
            _ => panic!("Expected InvalidPolicyContent error, got {:?}", error),
        }
    }

    // ============================================================================
    // DeletePolicyUseCase Tests
    // ============================================================================

    #[tokio::test]
    async fn test_delete_policy_success() {
        let mut policies = HashMap::new();
        let existing_policy = Policy {
            id: Hrn::new("iam", "policy", "existing-policy"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing policy".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        policies.insert("existing-policy".to_string(), existing_policy);

        let persister = Arc::new(MockPolicyPersister::with_policies(policies));
        let use_case = DeletePolicyUseCase::new(persister);

        let command = DeletePolicyCommand {
            policy_id: "existing-policy".to_string(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_delete_policy_not_found() {
        let persister = Arc::new(MockPolicyPersister::new());
        let use_case = DeletePolicyUseCase::new(persister);

        let command = DeletePolicyCommand {
            policy_id: "non-existent-policy".to_string(),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        match error {
            crate::features::create_policy::error::DeletePolicyError::PolicyNotFound => {}
            _ => panic!("Expected PolicyNotFound error, got {:?}", error),
        }
    }

    // ============================================================================
    // UpdatePolicyUseCase Tests
    // ============================================================================

    #[tokio::test]
    async fn test_update_policy_success() {
        let mut policies = HashMap::new();
        let existing_policy = Policy {
            id: Hrn::new("iam", "policy", "existing-policy"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing policy".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        policies.insert("existing-policy".to_string(), existing_policy);

        let persister = Arc::new(MockPolicyPersister::with_policies(policies));
        let validator = Arc::new(MockPolicyValidator::new_always_valid());
        let use_case = UpdatePolicyUseCase::new(persister, validator);

        let command = UpdatePolicyCommand {
            policy_id: "existing-policy".to_string(),
            policy_content: "forbid(principal, action, resource);".to_string(),
            description: Some("Updated policy".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());

        let policy_dto = result.unwrap();
        assert_eq!(policy_dto.id.to_string(), "hrn:iam:policy:existing-policy");
        assert_eq!(policy_dto.content, "forbid(principal, action, resource);");
        assert_eq!(policy_dto.description, Some("Updated policy".to_string()));
    }

    #[tokio::test]
    async fn test_update_policy_not_found() {
        let persister = Arc::new(MockPolicyPersister::new());
        let validator = Arc::new(MockPolicyValidator::new_always_valid());
        let use_case = UpdatePolicyUseCase::new(persister, validator);

        let command = UpdatePolicyCommand {
            policy_id: "non-existent-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test policy".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        match error {
            crate::features::create_policy::error::UpdatePolicyError::PolicyNotFound => {}
            _ => panic!("Expected PolicyNotFound error, got {:?}", error),
        }
    }

    #[tokio::test]
    async fn test_update_policy_invalid_content() {
        let mut policies = HashMap::new();
        let existing_policy = Policy {
            id: Hrn::new("iam", "policy", "existing-policy"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing policy".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        policies.insert("existing-policy".to_string(), existing_policy);

        let persister = Arc::new(MockPolicyPersister::with_policies(policies));
        let validator = Arc::new(MockPolicyValidator::new_always_invalid(vec![
            "Parse error: invalid syntax".to_string(),
        ]));
        let use_case = UpdatePolicyUseCase::new(persister, validator);

        let command = UpdatePolicyCommand {
            policy_id: "existing-policy".to_string(),
            policy_content: "invalid policy content".to_string(),
            description: Some("Test policy".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        match error {
            crate::features::create_policy::error::UpdatePolicyError::InvalidPolicyContent(msg) => {
                assert!(msg.contains("Parse error"));
            }
            _ => panic!("Expected InvalidPolicyContent error, got {:?}", error),
        }
    }

    // ============================================================================
    // GetPolicyUseCase Tests
    // ============================================================================

    #[tokio::test]
    async fn test_get_policy_success() {
        let mut policies = HashMap::new();
        let existing_policy = Policy {
            id: Hrn::new("iam", "policy", "existing-policy"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Existing policy".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        policies.insert("existing-policy".to_string(), existing_policy.clone());

        let persister = Arc::new(MockPolicyPersister::with_policies(policies));
        let use_case = GetPolicyUseCase::new(persister);

        let query = GetPolicyQuery {
            policy_id: "existing-policy".to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());

        let policy_dto = result.unwrap();
        assert_eq!(policy_dto.id, existing_policy.id);
        assert_eq!(policy_dto.content, existing_policy.content);
        assert_eq!(policy_dto.description, existing_policy.description);
    }

    #[tokio::test]
    async fn test_get_policy_not_found() {
        let persister = Arc::new(MockPolicyPersister::new());
        let use_case = GetPolicyUseCase::new(persister);

        let query = GetPolicyQuery {
            policy_id: "non-existent-policy".to_string(),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_err());

        let error = result.unwrap_err();
        match error {
            crate::features::create_policy::error::GetPolicyError::PolicyNotFound => {}
            _ => panic!("Expected PolicyNotFound error, got {:?}", error),
        }
    }

    // ============================================================================
    // ListPoliciesUseCase Tests
    // ============================================================================

    #[tokio::test]
    async fn test_list_policies_success() {
        let mut policies = HashMap::new();
        let policy1 = Policy {
            id: Hrn::new("iam", "policy", "policy-1"),
            content: "permit(principal, action, resource);".to_string(),
            description: Some("Policy 1".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let policy2 = Policy {
            id: Hrn::new("iam", "policy", "policy-2"),
            content: "forbid(principal, action, resource);".to_string(),
            description: Some("Policy 2".to_string()),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        policies.insert("policy-1".to_string(), policy1.clone());
        policies.insert("policy-2".to_string(), policy2.clone());

        let persister = Arc::new(MockPolicyPersister::with_policies(policies));
        let use_case = ListPoliciesUseCase::new(persister);

        let query = ListPoliciesQuery {
            limit: None,
            offset: None,
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());

        let policy_dtos = result.unwrap();
        assert_eq!(policy_dtos.len(), 2);
    }

    #[tokio::test]
    async fn test_list_policies_with_pagination() {
        let mut policies = HashMap::new();
        for i in 1..=10 {
            let policy = Policy {
                id: Hrn::new("iam", "policy", &format!("policy-{}", i)),
                content: "permit(principal, action, resource);".to_string(),
                description: Some(format!("Policy {}", i)),
                created_at: chrono::Utc::now(),
                updated_at: chrono::Utc::now(),
            };
            policies.insert(format!("policy-{}", i), policy);
        }

        let persister = Arc::new(MockPolicyPersister::with_policies(policies));
        let use_case = ListPoliciesUseCase::new(persister);

        // Test with limit
        let query = ListPoliciesQuery {
            limit: Some(5),
            offset: None,
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());
        let policy_dtos = result.unwrap();
        assert_eq!(policy_dtos.len(), 5);

        // Test with offset
        let query = ListPoliciesQuery {
            limit: None,
            offset: Some(5),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());
        let policy_dtos = result.unwrap();
        assert_eq!(policy_dtos.len(), 5);

        // Test with both limit and offset
        let query = ListPoliciesQuery {
            limit: Some(3),
            offset: Some(2),
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());
        let policy_dtos = result.unwrap();
        assert_eq!(policy_dtos.len(), 3);
    }

    #[tokio::test]
    async fn test_list_policies_empty() {
        let persister = Arc::new(MockPolicyPersister::new());
        let use_case = ListPoliciesUseCase::new(persister);

        let query = ListPoliciesQuery {
            limit: None,
            offset: None,
        };

        let result = use_case.execute(query).await;
        assert!(result.is_ok());

        let policy_dtos = result.unwrap();
        assert_eq!(policy_dtos.len(), 0);
    }
}
