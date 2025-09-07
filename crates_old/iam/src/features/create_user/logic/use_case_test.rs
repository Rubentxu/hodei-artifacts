use super::use_case::CreateUserUseCase;
use crate::application::ports::UserRepository;
use crate::error::IamError;
use crate::features::create_user::command::CreateUserCommand;
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
async fn test_create_user_use_case_success() {
    let mut mock_repo = MockUserRepositoryMock::new();
    mock_repo.expect_find_by_username()
        .with(predicate::eq("testuser"))
        .returning(|_| Ok(None));
    mock_repo.expect_save().returning(|_| Ok(()));

    let use_case = CreateUserUseCase::new(&mock_repo);

    let command = CreateUserCommand {
        username: "testuser".to_string(),
        email: "test@example.com".to_string(),
        password: "password123".to_string(),
        attributes: json!({}),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_create_user_use_case_username_exists() {
    let mut mock_repo = MockUserRepositoryMock::new();
    mock_repo.expect_find_by_username()
        .with(predicate::eq("existinguser"))
        .returning(|_| Ok(Some(User {
            id: UserId::new(),
            username: "existinguser".to_string(),
            email: "email@example.com".to_string(),
            password_hash: "hash".to_string(),
            status: UserStatus::Active,
            attributes: json!({}),
            groups: vec![],
            policies: vec![],
        })));
    // Expect save to not be called
    mock_repo.expect_save().times(0);

    let use_case = CreateUserUseCase::new(&mock_repo);

    let command = CreateUserCommand {
        username: "existinguser".to_string(),
        email: "test@example.com".to_string(),
        password: "password123".to_string(),
        attributes: json!({}),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), IamError::ValidationError(_)));
}

#[tokio::test]
async fn test_create_user_use_case_repository_save_failure() {
    let mut mock_repo = MockUserRepositoryMock::new();
    mock_repo.expect_find_by_username()
        .with(predicate::eq("testuser"))
        .returning(|_| Ok(None));
    mock_repo.expect_save().returning(|_| Err(IamError::InternalError("DB error".to_string())));

    let use_case = CreateUserUseCase::new(&mock_repo);

    let command = CreateUserCommand {
        username: "testuser".to_string(),
        email: "test@example.com".to_string(),
        password: "password123".to_string(),
        attributes: json!({}),
    };

    let result = use_case.execute(command).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), IamError::InternalError(_)));
}
