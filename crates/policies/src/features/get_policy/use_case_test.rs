//! Unit tests for get_policy use case
//!
//! This module contains comprehensive unit tests for the GetPolicyUseCase.

use chrono::Utc;
use shared::hrn::Hrn;
use std::sync::Arc;

use crate::domain::ids::{PolicyId};
use crate::domain::policy::{Policy, PolicyVersion};
use crate::features::get_policy::dto::GetPolicyQuery;
use crate::features::get_policy::error::GetPolicyError;
use crate::features::get_policy::mocks::{
    MockPolicyAccessValidator,
    MockPolicyRetrievalAuditor,
    MockPolicyRetrievalStorage,
};
use crate::features::get_policy::use_case::GetPolicyUseCase;

use std::str::FromStr;

#[cfg(test)]
mod tests {
    use super::*;
    use shared::hrn::UserId;

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

    fn create_test_query() -> GetPolicyQuery {
        GetPolicyQuery {
            policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            include_versions: false,
        }
    }

    #[tokio::test]
    async fn test_get_policy_success() {
        let mut mock_validator = MockPolicyAccessValidator::new();
        mock_validator
            .expect_validate_access()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut mock_storage = MockPolicyRetrievalStorage::new();
        mock_storage
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(Some(create_test_policy())));

        let mut mock_auditor = MockPolicyRetrievalAuditor::new();
        mock_auditor
            .expect_log_policy_access()
            .times(1)
            .returning(|_, _| Ok(()));

        let use_case = GetPolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let query = create_test_query();
        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.policy_id.as_str().contains("/policy/test-policy"));
        assert_eq!(response.name, "Test Policy");
        assert_eq!(response.status, "active");
        assert_eq!(response.version, 1);
    }

    #[tokio::test]
    async fn test_get_policy_with_versions() {
        let mut mock_validator = MockPolicyAccessValidator::new();
        mock_validator
            .expect_validate_access()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut mock_storage = MockPolicyRetrievalStorage::new();
        mock_storage
            .expect_find_by_id_with_versions()
            .times(1)
            .returning(|_| Ok(Some(create_test_policy())));

        let mut mock_auditor = MockPolicyRetrievalAuditor::new();
        mock_auditor
            .expect_log_policy_access()
            .times(1)
            .returning(|_, _| Ok(()));

        let use_case = GetPolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let mut query = create_test_query();
        query.include_versions = true;
        let user_id = UserId::from("test-user");
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policy_id.to_string(), "test-policy");
    }

    #[tokio::test]
    async fn test_get_policy_not_found() {
        let mut mock_validator = MockPolicyAccessValidator::new();
        mock_validator
            .expect_validate_access()
            .times(0); // Should not be called

        let mut mock_storage = MockPolicyRetrievalStorage::new();
        mock_storage
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(None)); // Policy not found

        let mock_auditor = MockPolicyRetrievalAuditor::new();

        let use_case = GetPolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let query = create_test_query();
        let user_id = UserId::from("test-user");
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            GetPolicyError::PolicyNotFound(_) => {},
            _ => panic!("Expected PolicyNotFound error"),
        }
    }

    #[tokio::test]
    async fn test_get_policy_access_denied() {
        let mut mock_validator = MockPolicyAccessValidator::new();
        mock_validator
            .expect_validate_access()
            .times(1)
            .returning(|_, _| Err(GetPolicyError::AuthorizationFailed));

        let mut mock_storage = MockPolicyRetrievalStorage::new();
        mock_storage
            .expect_find_by_id()
            .times(1)
            .returning(|_| Ok(Some(create_test_policy())));

        let mock_auditor = MockPolicyRetrievalAuditor::new();

        let use_case = GetPolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let query = create_test_query();
        let user_id = UserId::from("test-user");
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            GetPolicyError::AuthorizationFailed => {},
            _ => panic!("Expected AuthorizationFailed error"),
        }
    }

    #[tokio::test]
    async fn test_get_policy_storage_error() {
        let mock_validator = MockPolicyAccessValidator::new();

        let mut mock_storage = MockPolicyRetrievalStorage::new();
        mock_storage
            .expect_find_by_id()
            .times(1)
            .returning(|_| Err(GetPolicyError::storage_error("Storage failed")));

        let mock_auditor = MockPolicyRetrievalAuditor::new();

        let use_case = GetPolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let query = create_test_query();
        let user_id = UserId::from("test-user");
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            GetPolicyError::StorageError(_) => {},
            _ => panic!("Expected StorageError error"),
        }
    }

    #[tokio::test]
    async fn test_get_policy_details_success() {
        let mut mock_validator = MockPolicyAccessValidator::new();
        mock_validator
            .expect_validate_access()
            .times(1)
            .returning(|_, _| Ok(()));

        let mut mock_storage = MockPolicyRetrievalStorage::new();
        mock_storage
            .expect_find_by_id_with_versions()
            .times(1)
            .returning(|_| Ok(Some(create_test_policy())));

        let mut mock_auditor = MockPolicyRetrievalAuditor::new();
        mock_auditor
            .expect_log_policy_access()
            .times(1)
            .returning(|_, _| Ok(()));

        let use_case = GetPolicyUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
        );

        let policy_id = PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap();
        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.get_policy_details(&policy_id, &user_id).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.policy_id.as_str().contains("/policy/test-policy"));
        assert_eq!(response.name, "Test Policy");
    }
}
