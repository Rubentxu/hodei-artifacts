//! Unit tests for list_policies use case
//!
//! This module contains comprehensive unit tests for the ListPoliciesUseCase.

use chrono::Utc;
use shared::hrn::Hrn;
use std::sync::Arc;

use crate::domain::ids::{OrganizationId, PolicyId};
use crate::domain::policy::{Policy, PolicyVersion};
use crate::features::list_policies::dto::ListPoliciesQuery;
use crate::features::list_policies::error::ListPoliciesError;
use crate::features::list_policies::mocks::{
    MockListQueryValidator,
    MockPolicyListingAuditor,
    MockPolicyListingStorage,
};
use crate::features::list_policies::ports::ListPoliciesConfig;
use crate::features::list_policies::use_case::ListPoliciesUseCase;

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;
    use shared::hrn::UserId;

    fn create_test_policy() -> Policy {
        let now = Utc::now();
        Policy {
            id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy-1").unwrap(),
            name: "Test Policy 1".to_string(),
            description: Some("A test policy".to_string()),
            status: "active".to_string(),
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("hrn:hodei:iam::system:organization/test-org/policy/test-policy-1/versions/1").unwrap(),
                policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy-1").unwrap(),
                version: 1,
                content: r#"permit(principal, action == "read", resource);"#.to_string(),
                created_at: now,
                created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            },
        }
    }

    fn create_test_policies() -> Vec<Policy> {
        vec![
            create_test_policy(),
            Policy {
                id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy-2").unwrap(),
                name: "Test Policy 2".to_string(),
                description: Some("Another test policy".to_string()),
                status: "draft".to_string(),
                version: 1,
                created_at: Utc::now(),
                updated_at: Utc::now(),
                current_version: PolicyVersion {
                    id: Hrn::new("hrn:hodei:iam::system:organization/test-org/policy/test-policy-2/versions/1").unwrap(),
                    policy_id: PolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy-2").unwrap(),
                    version: 1,
                    content: r#"permit(principal, action == "write", resource);"#.to_string(),
                    created_at: Utc::now(),
                    created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
                },
            },
        ]
    }

    fn create_test_query() -> ListPoliciesQuery {
        ListPoliciesQuery {
            organization_id: Some(OrganizationId::from_str("hrn:hodei:iam::system:organization/test-org").unwrap()),
            name_filter: None,
            status_filter: None,
            created_by_filter: None,
            limit: Some(10),
            offset: Some(0),
            sort_by: Some("name".to_string()),
            sort_order: Some("asc".to_string()),
        }
    }

    #[tokio::test]
    async fn test_list_policies_success() {
        let mut mock_validator = MockListQueryValidator::new();
        mock_validator
            .expect_validate_query()
            .times(1)
            .returning(|_, _| Ok(()));
        mock_validator
            .expect_apply_access_filter()
            .times(1)
            .returning(|query, _| Ok(query.clone()));

        let mut mock_storage = MockPolicyListingStorage::new();
        let test_policies = create_test_policies();
        mock_storage
            .expect_find_all()
            .times(1)
            .returning(move |_| Ok(test_policies.clone()));
        mock_storage
            .expect_count()
            .times(1)
            .returning(|_| Ok(2));

        let mut mock_auditor = MockPolicyListingAuditor::new();
        mock_auditor
            .expect_log_policy_list_access()
            .times(1)
            .returning(|_, _, _| Ok(()));

        let config = ListPoliciesConfig::default();
        let use_case = ListPoliciesUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
            config,
        );

        let query = create_test_query();
        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 2);
        assert_eq!(response.total_count, 2);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_list_policies_empty_result() {
        let mut mock_validator = MockListQueryValidator::new();
        mock_validator
            .expect_validate_query()
            .times(1)
            .returning(|_, _| Ok(()));
        mock_validator
            .expect_apply_access_filter()
            .times(1)
            .returning(|query, _| Ok(query.clone()));

        let mut mock_storage = MockPolicyListingStorage::new();
        mock_storage
            .expect_find_all()
            .times(1)
            .returning(|_| Ok(vec![]));
        mock_storage
            .expect_count()
            .times(1)
            .returning(|_| Ok(0));

        let mut mock_auditor = MockPolicyListingAuditor::new();
        mock_auditor
            .expect_log_policy_list_access()
            .times(1)
            .returning(|_, _, _| Ok(()));

        let config = ListPoliciesConfig::default();
        let use_case = ListPoliciesUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
            config,
        );

        let query = create_test_query();
        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 0);
        assert_eq!(response.total_count, 0);
        assert!(!response.has_more);
    }

    #[tokio::test]
    async fn test_list_policies_validation_error() {
        let mut mock_validator = MockListQueryValidator::new();
        mock_validator
            .expect_validate_query()
            .times(1)
            .returning(|_, _| Err(ListPoliciesError::invalid_query("limit", "Invalid limit")));

        let mock_storage = MockPolicyListingStorage::new();
        let mock_auditor = MockPolicyListingAuditor::new();

        let config = ListPoliciesConfig::default();
        let use_case = ListPoliciesUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
            config,
        );

        let query = create_test_query();
        let user_id = UserId::from("test-user");
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            ListPoliciesError::InvalidQueryParameters { .. } => {},
            _ => panic!("Expected InvalidQueryParameters error"),
        }
    }

    #[tokio::test]
    async fn test_list_policies_storage_error() {
        let mut mock_validator = MockListQueryValidator::new();
        mock_validator
            .expect_validate_query()
            .times(1)
            .returning(|_, _| Ok(()));
        mock_validator
            .expect_apply_access_filter()
            .times(1)
            .returning(|query, _| Ok(query.clone()));

        let mut mock_storage = MockPolicyListingStorage::new();
        mock_storage
            .expect_find_all()
            .times(1)
            .returning(|_| Err(ListPoliciesError::storage_error("Storage failed")));

        let mock_auditor = MockPolicyListingAuditor::new();

        let config = ListPoliciesConfig::default();
        let use_case = ListPoliciesUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
            config,
        );

        let query = create_test_query();
        let user_id = UserId::from("test-user");
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            ListPoliciesError::StorageError(_) => {},
            _ => panic!("Expected StorageError error"),
        }
    }

    #[tokio::test]
    async fn test_list_policies_with_pagination() {
        let mut mock_validator = MockListQueryValidator::new();
        mock_validator
            .expect_validate_query()
            .times(1)
            .returning(|_, _| Ok(()));
        mock_validator
            .expect_apply_access_filter()
            .times(1)
            .returning(|query, _| Ok(query.clone()));

        let mut mock_storage = MockPolicyListingStorage::new();
        let test_policies = create_test_policies();
        mock_storage
            .expect_find_all()
            .times(1)
            .returning(move |_| Ok(test_policies.clone()));
        mock_storage
            .expect_count()
            .times(1)
            .returning(|_| Ok(5)); // More total than returned

        let mut mock_auditor = MockPolicyListingAuditor::new();
        mock_auditor
            .expect_log_policy_list_access()
            .times(1)
            .returning(|_, _, _| Ok(()));

        let config = ListPoliciesConfig::default();
        let use_case = ListPoliciesUseCase::new(
            Arc::new(mock_validator),
            Arc::new(mock_storage),
            Arc::new(mock_auditor),
            config,
        );

        let mut query = create_test_query();
        query.limit = Some(2);
        query.offset = Some(0);
        let user_id = UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap();
        let result = use_case.execute(query, &user_id).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.policies.len(), 2);
        assert_eq!(response.total_count, 5);
        assert!(response.has_more); // Because total_count > returned count
    }
}
