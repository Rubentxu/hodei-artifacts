use super::use_case::UpdateUserAttributesUseCase;
use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::update_user_attributes::command::UpdateUserAttributesCommand;
use async_trait::async_trait;
use mockall::{mock, predicate::*};
use mockall::predicate;
use shared::UserId;
use crate::domain::user::{User, UserStatus};
use serde_json::json;

mock! {
    UserRepositoryMock {}
    #[async_trait]
    impl UserRepository for UserRepositoryMock {
        async fn save(&self, user: &User) -> Result<(), IamError>;
        async fn find_by_id(&self, id: &UserId) -> Result<Option<User>, IamError>;
        async fn find_by_username(&self, username: &str) -> Result<Option<User>, IamError>;
        async fn find_all(&self) -> Result<Vec<User>, IamError>;
        async fn delete(&self, id: &UserId) -> Result<(), IamError>;
        
    }
}

#[tokio::test]
async fn test_update_user_attributes_use_case_success() {
    let mut mock_repo = MockUserRepositoryMock::new();
    let user_id = UserId::new();
    let mut existing_user = User {
            id: user_id.clone(),
            username: "testuser".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            status: UserStatus::Active,
            attributes: json!({}),
            groups: vec![],
            policies: vec![],
        };

    mock_repo.expect_find_by_id()
        .with(predicate::eq(user_id.clone()))
        .returning(move |_| Ok(Some(existing_user.clone())));
    mock_repo.expect_save().returning(|_| Ok(()));

    let use_case = UpdateUserAttributesUseCase::new(&mock_repo);

    let command = UpdateUserAttributesCommand {
        user_id: user_id.clone(),
        attributes: json!({"new_attribute": "value"}),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_update_user_attributes_use_case_user_not_found() {
    let mut mock_repo = MockUserRepositoryMock::new();
    let user_id = UserId::new();

    mock_repo.expect_find_by_id()
        .with(predicate::eq(user_id.clone()))
        .returning(|_| Ok(None));
    mock_repo.expect_save().times(0); // Should not be called

    let use_case = UpdateUserAttributesUseCase::new(&mock_repo);

    let command = UpdateUserAttributesCommand {
        user_id: user_id.clone(),
        attributes: json!({"new_attribute": "value"}),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), IamError::NotFound));
}

#[tokio::test]
async fn test_update_user_attributes_use_case_repository_save_failure() {
    let mut mock_repo = MockUserRepositoryMock::new();
    let user_id = UserId::new();
    let existing_user = User {
            id: user_id.clone(),
            username: "testuser".to_string(),
            email: "test@example.com".to_string(),
            password_hash: "hash".to_string(),
            status: UserStatus::Active,
            attributes: json!({}),
            groups: vec![],
            policies: vec![],
        };

    mock_repo.expect_find_by_id()
        .with(predicate::eq(user_id.clone()))
        .returning(move |_| Ok(Some(existing_user.clone())));
    mock_repo.expect_save().returning(|_| Err(IamError::InternalError("DB error".to_string())));

    let use_case = UpdateUserAttributesUseCase::new(&mock_repo);

    let command = UpdateUserAttributesCommand {
        user_id: user_id.clone(),
        attributes: json!({"new_attribute": "value"}),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), IamError::InternalError(_)));
}
