use chrono::Utc;
use std::str::FromStr;
use std::sync::Arc;

use crate::domain::ids::{OrganizationId};
use crate::domain::policy::{Policy, PolicyVersion};
use crate::features::create_policy::dto::CreatePolicyCommand;
use crate::features::create_policy::error::CreatePolicyError;
use crate::features::create_policy::ports::{PolicyCreationValidator, PolicyCreationStorage, PolicyExistenceChecker, PolicyCreationAuditor};
use crate::features::create_policy::use_case::CreatePolicyUseCase;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy::mocks::{
        MockPolicyCreationAuditor,
        MockPolicyCreationStorage,
        MockPolicyCreationValidator,
        MockPolicyExistenceChecker,
    };
    use std::str::FromStr;
    use cedar_policy::PolicyId;
    use shared::hrn::{UserId, Hrn, HodeiPolicyId};

    fn create_test_command() -> CreatePolicyCommand {
        CreatePolicyCommand {
            policy_id: HodeiPolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            name: "Test Policy".to_string(),
            description: Some("A test policy".to_string()),
            organization_id: OrganizationId::from_str("hrn:hodei:iam::system:organization/test-org").unwrap(),
            content: r#"permit(principal, action == "read", resource);"#.to_string(),
            created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
        }
    }

    fn create_test_policy() -> Policy {
        let now = Utc::now();
        Policy {
            id: HodeiPolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
            name: "Test Policy".to_string(),
            description: Some("A test policy".to_string()),
            status: "active".to_string(),
            version: 1,
            created_at: now,
            updated_at: now,
            current_version: PolicyVersion {
                id: Hrn::new("hrn:hodei:iam::system:organization/test-org/policy/test-policy/versions/1").unwrap(),
                policy_id: HodeiPolicyId::from_str("hrn:hodei:iam::system:organization/test-org/policy/test-policy").unwrap(),
                version: 1,
                content: r#"permit(principal, action == "read", resource);"#.to_string(),
                created_at: now,
                created_by: UserId::from_str("hrn:hodei:iam::system:user/test-user").unwrap(),
            },
        }
    }

    #[tokio::test]
    async fn test_create_policy_success() {
        let validator = MockPolicyCreationValidator::new()
            .with_validate_policy_content_result(Ok(()))
            .with_validate_policy_syntax_result(Ok(()))
            .with_validate_policy_semantics_result(Ok(()));

        let existence_checker = MockPolicyExistenceChecker::new()
            .with_exists_result(Ok(false));

        let storage = MockPolicyCreationStorage::new()
            .with_save_result(Ok(()))
            .with_create_version_result(Ok(()));

        let auditor = MockPolicyCreationAuditor::new()
            .with_log_policy_creation_result(Ok(()));

        let use_case = CreatePolicyUseCase::new(
            Arc::new(validator),
            Arc::new(existence_checker),
            Arc::new(storage),
            Arc::new(auditor),
        );

        let command = create_test_command();
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.name, "Test Policy");
        assert_eq!(response.status, "active");
        assert_eq!(response.version, 1);
    }

    #[tokio::test]
    async fn test_create_policy_validation_failure() {
        let validator = MockPolicyCreationValidator::new()
            .with_validate_policy_content_result(Err(CreatePolicyError::validation_failed("Invalid content")));

        let existence_checker = MockPolicyExistenceChecker::new();
        let storage = MockPolicyCreationStorage::new();
        let auditor = MockPolicyCreationAuditor::new();

        let use_case = CreatePolicyUseCase::new(
            Arc::new(validator),
            Arc::new(existence_checker),
            Arc::new(storage),
            Arc::new(auditor),
        );

        let command = create_test_command();
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            CreatePolicyError::PolicyValidationFailed(_) => {},
            _ => panic!("Expected validation error"),
        }
    }

    #[tokio::test]
    async fn test_create_policy_already_exists() {
        let validator = MockPolicyCreationValidator::new()
            .with_validate_policy_content_result(Ok(()))
            .with_validate_policy_syntax_result(Ok(()))
            .with_validate_policy_semantics_result(Ok(()));

        let existence_checker = MockPolicyExistenceChecker::new()
            .with_exists_result(Ok(true)); // Policy already exists

        let storage = MockPolicyCreationStorage::new();
        let auditor = MockPolicyCreationAuditor::new();

        let use_case = CreatePolicyUseCase::new(
            Arc::new(validator),
            Arc::new(existence_checker),
            Arc::new(storage),
            Arc::new(auditor),
        );

        let command = create_test_command();
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            CreatePolicyError::PolicyAlreadyExists(_) => {},
            _ => panic!("Expected already exists error"),
        }
    }

    #[tokio::test]
    async fn test_create_policy_storage_failure() {
        let validator = MockPolicyCreationValidator::new()
            .with_validate_policy_content_result(Ok(()))
            .with_validate_policy_syntax_result(Ok(()))
            .with_validate_policy_semantics_result(Ok(()));

        let existence_checker = MockPolicyExistenceChecker::new()
            .with_exists_result(Ok(false));

        let storage = MockPolicyCreationStorage::new()
            .with_save_result(Err(CreatePolicyError::storage_error("Save failed")));

        let auditor = MockPolicyCreationAuditor::new();

        let use_case = CreatePolicyUseCase::new(
            Arc::new(validator),
            Arc::new(existence_checker),
            Arc::new(storage),
            Arc::new(auditor),
        );

        let command = create_test_command();
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        match error {
            CreatePolicyError::StorageError(_) => {},
            _ => panic!("Expected storage error"),
        }
    }
}