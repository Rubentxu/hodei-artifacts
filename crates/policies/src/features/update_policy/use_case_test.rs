//! Unit tests for update_policy use case
//!
//! This module contains unit tests for the UpdatePolicyUseCase.

use chrono::Utc;
use mockall::predicate::*;
use std::str::FromStr;
use std::sync::Arc;

use crate::domain::ids::PolicyId;
use crate::domain::policy::{Policy, PolicyVersion};
use crate::features::update_policy::dto::{UpdatePolicyCommand, UpdatePolicyResponse};
use crate::features::update_policy::error::UpdatePolicyError;
use crate::features::update_policy::ports::{
    PolicyRetriever, PolicyUpdateAuditor, PolicyUpdateStorage, PolicyUpdateValidator
};
use crate::features::update_policy::use_case::UpdatePolicyUseCase;
use shared::hrn::{Hrn, UserId};

/// Create a test policy for use in tests
fn create_test_policy() -> Policy {
    let now = Utc::now();
    Policy {
        id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
        name: "Test Policy".to_string(),
        description: Some("A test policy".to_string()),
        status: "active".to_string(),
        version: 1,
        created_at: now,
        updated_at: now,
        current_version: PolicyVersion {
            id: Hrn::new("hrn:hodei:iam::system:organization/test-org/policy/test-policy/versions/1").unwrap(),
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            version: 1,
            content: r#"permit(principal, action == "read", resource);"#.to_string(),
            created_at: now,
            created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        },
    }
}

/// Create a test command for use in tests
fn create_test_command() -> UpdatePolicyCommand {
    UpdatePolicyCommand {
        name: Some("Updated Test Policy".to_string()),
        description: Some("An updated test policy".to_string()),
        content: Some(r#"permit(principal, action == "write", resource);"#.to_string()),
        expected_version: Some(1),
        updated_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::update_policy::mocks::{
        MockPolicyRetriever, MockPolicyUpdateAuditor, MockPolicyUpdateStorage, MockPolicyUpdateValidator
    };

    #[tokio::test]
    async fn test_update_policy_success() {
        let validator = MockPolicyUpdateValidator::new()
            .with_validate_update_allowed_result(Ok(()))
            .with_validate_policy_content_result(Ok(()))
            .with_validate_policy_syntax_result(Ok(()))
            .with_validate_policy_semantics_result(Ok(()));

        let retriever = MockPolicyRetriever::new()
            .with_get_policy_result(Ok(Some(create_test_policy())));

        let storage = MockPolicyUpdateStorage::new()
            .with_update_result(Ok(()))
            .with_create_version_result(Ok(()));

        let auditor = MockPolicyUpdateAuditor::new()
            .with_log_policy_update_result(Ok(()));

        let use_case = UpdatePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap();
        let command = create_test_command();
        let result = use_case.execute(&policy_id, command).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.name, "Updated Test Policy");
        assert_eq!(response.version, 2); // Version should be incremented
    }

    #[tokio::test]
    async fn test_update_policy_not_found() {
        let validator = MockPolicyUpdateValidator::new();
        let retriever = MockPolicyRetriever::new()
            .with_get_policy_result(Ok(None)); // Policy not found

        let storage = MockPolicyUpdateStorage::new();
        let auditor = MockPolicyUpdateAuditor::new();

        let use_case = UpdatePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/non-existent").unwrap();
        let command = create_test_command();
        let result = use_case.execute(&policy_id, command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            UpdatePolicyError::PolicyNotFound(_) => {},
            _ => panic!("Expected PolicyNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_version_conflict() {
        let mock_validator = MockPolicyUpdateValidator::new();
        let mut mock_retriever = MockPolicyRetriever::new();
        mock_retriever
            .expect_get_policy()
            .times(1)
            .returning(|_| Ok(Some(create_test_policy())));

        let mock_storage = MockPolicyUpdateStorage::new();
        let mock_auditor = MockPolicyUpdateAuditor::new();

        let use_case = UpdatePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap();
        let mut command = create_test_command();
        command.expected_version = Some(2); // Wrong expected version
        let result = use_case.execute(&policy_id, command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            UpdatePolicyError::VersionMismatch { .. } => {},
            _ => panic!("Expected VersionMismatch error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_validation_failure() {
        let validator = MockPolicyUpdateValidator::new()
            .with_validate_update_allowed_result(Ok(()))
            .with_validate_policy_content_result(Err(UpdatePolicyError::validation_failed("Invalid content")));

        let retriever = MockPolicyRetriever::new()
            .with_get_policy_result(Ok(Some(create_test_policy())));

        let storage = MockPolicyUpdateStorage::new();
        let auditor = MockPolicyUpdateAuditor::new();

        let use_case = UpdatePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap();
        let command = create_test_command();
        let result = use_case.execute(&policy_id, command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            UpdatePolicyError::PolicyValidationFailed(_) => {},
            _ => panic!("Expected validation error"),
        }
    }

    #[tokio::test]
    async fn test_update_policy_storage_failure() {
        let validator = MockPolicyUpdateValidator::new()
            .with_validate_update_allowed_result(Ok(()))
            .with_validate_policy_content_result(Ok(()))
            .with_validate_policy_syntax_result(Ok(()))
            .with_validate_policy_semantics_result(Ok(()));

        let retriever = MockPolicyRetriever::new()
            .with_get_policy_result(Ok(Some(create_test_policy())));

        let storage = MockPolicyUpdateStorage::new()
            .with_update_result(Err(UpdatePolicyError::storage_error("Update failed")));

        let auditor = MockPolicyUpdateAuditor::new();

        let use_case = UpdatePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap();
        let command = create_test_command();
        let result = use_case.execute(&policy_id, command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            UpdatePolicyError::StorageError(_) => {},
            _ => panic!("Expected storage error"),
        }
    }
}