use super::use_case::CreatePolicyUseCase;
use crate::application::ports::{PolicyRepository, PolicyValidator};
use crate::error::IamError;
use crate::features::create_policy::command::CreatePolicyCommand;
use async_trait::async_trait;
use mockall::{mock, predicate::*};
use mockall::predicate;
use cedar_policy::PolicyId;
use std::sync::Arc;
use std::collections::HashSet;

mock! {
    PolicyRepositoryMock {}
    #[async_trait]
    impl PolicyRepository for PolicyRepositoryMock {
        async fn save(&self, policy: &crate::domain::Policy) -> Result<(), IamError>;
        async fn find_by_id(&self, id: &PolicyId) -> Result<Option<crate::domain::Policy>, IamError>;
        async fn find_all(&self) -> Result<Vec<crate::domain::Policy>, IamError>;
        async fn delete(&self, id: &PolicyId) -> Result<(), IamError>;
    }
}

mock! {
    PolicyValidatorMock {}
    impl PolicyValidator for PolicyValidatorMock {
        fn validate_policy_syntax(&self, policy_content: &str) -> Result<(), IamError>;
        fn validate_policy_semantics(&self, policy_content: &str, entities: HashSet<String>) -> Result<(), IamError>;
    }
}

#[tokio::test]
async fn test_create_policy_use_case_success() {
    let mut mock_repo = MockPolicyRepositoryMock::new();
    mock_repo.expect_save().returning(|_| Ok(()));

    let mut mock_validator = MockPolicyValidatorMock::new();
    mock_validator.expect_validate_policy_syntax().returning(|_| Ok(()));
    mock_validator.expect_validate_policy_semantics().returning(|_, _| Ok(()));

    let use_case = CreatePolicyUseCase::new(
        &mock_repo,
        &mock_validator,
    );

    let command = CreatePolicyCommand {
        name: "test_policy".to_string(),
        description: None,
        content: "permit(principal, action, resource);".to_string(),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_create_policy_use_case_validation_failure() {
    let mock_repo = MockPolicyRepositoryMock::new(); // No expectations needed for save

    let mut mock_validator = MockPolicyValidatorMock::new();
    mock_validator.expect_validate_policy_syntax().returning(|_| Err(IamError::ValidationError("Invalid syntax".to_string())));
    // Semantics validation won't be called if syntax validation fails

    let use_case = CreatePolicyUseCase::new(
        &mock_repo,
        &mock_validator,
    );

    let command = CreatePolicyCommand {
        name: "test_policy".to_string(),
        description: None,
        content: "invalid policy content".to_string(),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), IamError::ValidationError(_)));
}

#[tokio::test]
async fn test_create_policy_use_case_repository_save_failure() {
    let mut mock_repo = MockPolicyRepositoryMock::new();
    mock_repo.expect_save().returning(|_| Err(IamError::InternalError("DB error".to_string())));

    let mut mock_validator = MockPolicyValidatorMock::new();
    mock_validator.expect_validate_policy_syntax().returning(|_| Ok(()));
    mock_validator.expect_validate_policy_semantics().returning(|_, _| Ok(()));

    let use_case = CreatePolicyUseCase::new(
        &mock_repo,
        &mock_validator,
    );

    let command = CreatePolicyCommand {
        name: "test_policy".to_string(),
        description: None,
        content: "permit(principal, action, resource);".to_string(),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), IamError::InternalError(_)));
}
