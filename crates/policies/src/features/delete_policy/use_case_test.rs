//! Unit tests for delete_policy use case
//!
//! This module contains comprehensive unit tests for the DeletePolicyUseCase.

use chrono::Utc;
use shared::hrn::Hrn;
use std::sync::Arc;

use crate::domain::ids::{PolicyId};
use crate::domain::policy::{Policy, PolicyVersion};
use crate::features::delete_policy::dto::DeletePolicyCommand;
use crate::features::delete_policy::error::DeletePolicyError;
use crate::features::delete_policy::mocks::{
    MockPolicyDeletionValidator,
    MockPolicyDeletionRetriever,
    MockPolicyDeletionStorage,
    MockPolicyDeletionAuditor,
};
use crate::features::delete_policy::ports::DeletionMode;
use crate::features::delete_policy::use_case::DeletePolicyUseCase;

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use shared::hrn::UserId;

    fn create_test_command() -> DeletePolicyCommand {
        DeletePolicyCommand {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            deleted_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            deletion_mode: DeletionMode::Soft,
            reason: Some("Test deletion".to_string()),
        }
    }

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

    #[tokio::test]
    async fn test_delete_policy_soft_success() {
        let mut mock_validator = MockPolicyDeletionValidator::new();
        mock_validator
            .expect_validate_deletion_allowed()
            .times(1)
            .returning(|_, _| Ok(()));
        mock_validator
            .expect_check_dependencies()
            .times(1)
            .returning(|_| Ok(()));

        let mut mock_retriever = MockPolicyDeletionRetriever::new();
        mock_retriever
            .expect_get_policy()
            .times(1)
            .returning(|_| Ok(Some(create_test_policy())));

        let mut mock_storage = MockPolicyDeletionStorage::new();
        mock_storage
            .expect_soft_delete()
            .times(1)
            .returning(|_| Ok(()));
        mock_storage
            .expect_hard_delete()
            .times(0); // Should not be called for soft delete
        mock_storage
            .expect_archive_versions()
            .times(0); // Should not be called for soft delete

        let mut mock_auditor = MockPolicyDeletionAuditor::new();
        mock_auditor
            .expect_log_policy_deletion()
            .times(1)
            .returning(|_, _| Ok(()));

        let use_case = DeletePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let command = create_test_command();
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy_id.to_string(), "test-policy");
        assert_eq!(response.deletion_mode, DeletionMode::Soft);
        assert!(response.success);
        assert_eq!(response.message, "Policy soft deleted successfully");
    }

    #[tokio::test]
    async fn test_delete_policy_hard_success() {
        let mut mock_validator = MockPolicyDeletionValidator::new();
        mock_validator
            .expect_validate_deletion_allowed()
            .times(1)
            .returning(|_, _| Ok(()));
        mock_validator
            .expect_check_dependencies()
            .times(1)
            .returning(|_| Ok(()));

        let mut mock_retriever = MockPolicyDeletionRetriever::new();
        mock_retriever
            .expect_get_policy()
            .times(1)
            .returning(|_| Ok(Some(create_test_policy())));

        let mut mock_storage = MockPolicyDeletionStorage::new();
        mock_storage
            .expect_soft_delete()
            .times(0); // Should not be called for hard delete
        mock_storage
            .expect_hard_delete()
            .times(1)
            .returning(|_| Ok(()));
        mock_storage
            .expect_archive_versions()
            .times(1)
            .returning(|_| Ok(()));

        let mut mock_auditor = MockPolicyDeletionAuditor::new();
        mock_auditor
            .expect_log_policy_deletion()
            .times(1)
            .returning(|_, _| Ok(()));

        let use_case = DeletePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let mut command = create_test_command();
        command.deletion_mode = DeletionMode::Hard;
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.deletion_mode, DeletionMode::Hard);
        assert_eq!(response.message, "Policy permanently deleted");
    }

    #[tokio::test]
    async fn test_delete_policy_not_found() {
        let mut mock_validator = MockPolicyDeletionValidator::new();
        mock_validator
            .expect_validate_deletion_allowed()
            .times(0); // Should not be called

        let mut mock_retriever = MockPolicyDeletionRetriever::new();
        mock_retriever
            .expect_get_policy()
            .times(1)
            .returning(|_| Ok(None)); // Policy not found

        let mock_storage = MockPolicyDeletionStorage::new();
        let mock_auditor = MockPolicyDeletionAuditor::new();

        let use_case = DeletePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let command = create_test_command();
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            DeletePolicyError::PolicyNotFound(_) => {},
            _ => panic!("Expected PolicyNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_delete_policy_validation_failure() {
        let mut mock_validator = MockPolicyDeletionValidator::new();
        mock_validator
            .expect_validate_deletion_allowed()
            .times(1)
            .returning(|_, _| Err(DeletePolicyError::deletion_not_allowed("Cannot delete system policy")));

        let mut mock_retriever = MockPolicyDeletionRetriever::new();
        mock_retriever
            .expect_get_policy()
            .times(1)
            .returning(|_| Ok(Some(create_test_policy())));

        let mock_storage = MockPolicyDeletionStorage::new();
        let mock_auditor = MockPolicyDeletionAuditor::new();

        let use_case = DeletePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let command = create_test_command();
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            DeletePolicyError::PolicyDeletionNotAllowed(_) => {},
            _ => panic!("Expected PolicyDeletionNotAllowed error"),
        }
    }

    #[tokio::test]
    async fn test_delete_policy_dependencies_failure() {
        let mut mock_validator = MockPolicyDeletionValidator::new();
        mock_validator
            .expect_validate_deletion_allowed()
            .times(1)
            .returning(|_, _| Ok(()));
        mock_validator
            .expect_check_dependencies()
            .times(1)
            .returning(|_| Err(DeletePolicyError::has_dependencies("Policy has dependencies")));

        let mut mock_retriever = MockPolicyDeletionRetriever::new();
        mock_retriever
            .expect_get_policy()
            .times(1)
            .returning(|_| Ok(Some(create_test_policy())));

        let mock_storage = MockPolicyDeletionStorage::new();
        let mock_auditor = MockPolicyDeletionAuditor::new();

        let use_case = DeletePolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_retriever),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let command = create_test_command();
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            DeletePolicyError::PolicyHasDependencies(_) => {},
            _ => panic!("Expected PolicyHasDependencies error"),
        }
    }
}
