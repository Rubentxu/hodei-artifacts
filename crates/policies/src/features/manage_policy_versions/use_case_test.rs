//! Unit tests for manage_policy_versions use case
//!
//! This module contains comprehensive unit tests for the ManagePolicyVersionsUseCase.

use chrono::Utc;
use shared::hrn::Hrn;
use std::sync::Arc;

use crate::domain::ids::PolicyId;
use crate::domain::policy::PolicyVersion;
use crate::features::manage_policy_versions::dto::{CreatePolicyVersionCommand, GetPolicyVersionsQuery};
use crate::features::manage_policy_versions::error::ManagePolicyVersionsError;
use crate::features::manage_policy_versions::mocks::{
    MockPolicyVersionAuditor,
    MockPolicyVersionHistory,
    MockPolicyVersionStorage,
    MockPolicyVersionValidator,
};
use crate::features::manage_policy_versions::use_case::ManagePolicyVersionsUseCase;


#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::UserId;
    use std::str::FromStr;

    fn create_test_version() -> PolicyVersion {
        let now = Utc::now();
        PolicyVersion {
            id: Hrn::new("hrn:hodei:iam::system:organization/test-org/policy/test-policy/versions/1").unwrap(),
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            version: 1,
            content: r#"permit(principal, action == "read", resource);"#.to_string(),
            created_at: now,
            created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        }
    }

    fn create_test_command() -> CreatePolicyVersionCommand {
        CreatePolicyVersionCommand {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            content: r#"permit(principal, action == "write", resource);"#.to_string(),
            created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        }
    }

    #[tokio::test]
    async fn test_create_version_success() {
        let mut mock_validator = MockPolicyVersionValidator::new();
        mock_validator
            .expect_validate_version_number()
            .times(1)
            .returning(|_| Ok(()));
        mock_validator
            .expect_validate_version_content()
            .times(1)
            .returning(|_| Ok(()));

        let mut mock_history = MockPolicyVersionHistory::new();
        mock_history
            .expect_get_version_history()
            .times(0); // Not called in create_version

        let mut mock_storage = MockPolicyVersionStorage::new();
        mock_storage
            .expect_find_versions_by_policy()
            .times(1)
            .returning(|_| Ok(vec![create_test_version()]));
        mock_storage
            .expect_save_version()
            .times(1)
            .returning(|_| Ok(()));

        let mut mock_auditor = MockPolicyVersionAuditor::new();
        mock_auditor
            .expect_log_version_creation()
            .times(1)
            .returning(|_, _, _| Ok(()));

        let use_case = ManagePolicyVersionsUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_history),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let command = create_test_command();
        let result = use_case.create_version(command).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.policy_id.as_str().contains("/policy/test-policy"));
        assert_eq!(response.version, 2); // Next version after existing version 1
    }

    #[tokio::test]
    async fn test_create_version_validation_failure() {
        let mut mock_validator = MockPolicyVersionValidator::new();
        mock_validator
            .expect_validate_version_content()
            .times(1)
            .returning(|_| Err(ManagePolicyVersionsError::history_error("Invalid content")));

        let mock_history = MockPolicyVersionHistory::new();
        let mock_storage = MockPolicyVersionStorage::new();
        let mock_auditor = MockPolicyVersionAuditor::new();

        let use_case = ManagePolicyVersionsUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_history),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let command = create_test_command();
        let result = use_case.create_version(command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            ManagePolicyVersionsError::VersionHistoryError(_) => {},
            _ => panic!("Expected VersionHistoryError"),
        }
    }

    #[tokio::test]
    async fn test_get_versions_success() {
        let mock_validator = MockPolicyVersionValidator::new();

        let mut mock_history = MockPolicyVersionHistory::new();
        let test_versions = vec![create_test_version()];
        mock_history
            .expect_get_version_history()
            .times(1)
            .returning(move |_, _| Ok(test_versions.clone()));

        let mock_storage = MockPolicyVersionStorage::new();
        let mock_auditor = MockPolicyVersionAuditor::new();

        let use_case = ManagePolicyVersionsUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_history),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let query = GetPolicyVersionsQuery {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy"),
            limit: Some(10),
            offset: None,
        };

        let result = use_case.get_versions(query).await;

        assert!(result.is_ok());
        let versions = result.unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, 1);
    }

    #[tokio::test]
    async fn test_get_version_history_success() {
        let mock_validator = MockPolicyVersionValidator::new();

        let mut mock_history = MockPolicyVersionHistory::new();
        let test_versions = vec![create_test_version()];
        mock_history
            .expect_get_version_history()
            .times(1)
            .returning(move |_, _| Ok(test_versions.clone()));

        let mock_storage = MockPolicyVersionStorage::new();
        let mock_auditor = MockPolicyVersionAuditor::new();

        let use_case = ManagePolicyVersionsUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_history),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy");
        let result = use_case.get_version_history(&policy_id, Some(5)).await;

        assert!(result.is_ok());
        let versions = result.unwrap();
        assert_eq!(versions.len(), 1);
    }

    #[tokio::test]
    async fn test_compare_versions_success() {
        let mock_validator = MockPolicyVersionValidator::new();

        let mut mock_history = MockPolicyVersionHistory::new();
        mock_history
            .expect_get_version_diff()
            .times(1)
            .returning(|_, _, _| Ok("Version differences here".to_string()));

        let mock_storage = MockPolicyVersionStorage::new();
        let mock_auditor = MockPolicyVersionAuditor::new();

        let use_case = ManagePolicyVersionsUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_history),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy");
        let result = use_case.compare_versions(&policy_id, 1, 2).await;

        assert!(result.is_ok());
        let diff = result.unwrap();
        assert_eq!(diff, "Version differences here");
    }

    #[tokio::test]
    async fn test_storage_error() {
        let mock_validator = MockPolicyVersionValidator::new();

        let mut mock_history = MockPolicyVersionHistory::new();
        mock_history
            .expect_get_version_history()
            .times(1)
            .returning(|_, _| Err(ManagePolicyVersionsError::storage_error("Storage failed")));

        let mock_storage = MockPolicyVersionStorage::new();
        let mock_auditor = MockPolicyVersionAuditor::new();

        let use_case = ManagePolicyVersionsUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_history),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy");
        let result = use_case.get_version_history(&policy_id, None).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            ManagePolicyVersionsError::StorageError(_) => {},
            _ => panic!("Expected StorageError"),
        }
    }
}
