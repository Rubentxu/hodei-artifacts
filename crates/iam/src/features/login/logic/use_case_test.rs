#[cfg(test)]
mod tests {
    use crate::features::login::logic::use_case::LoginUseCase;
    use crate::features::login::command::LoginCommand;
    use crate::mocks::user_repository::MockUserRepository;
    use crate::application::ports::UserRepository;
    use crate::domain::user::{User, UserStatus};
    use shared::UserId;

    #[tokio::test]
    async fn test_login_success() {
        let user_repo = MockUserRepository::new();
        let user_id = UserId::new();
        let password = "password123";
        let password_hash = bcrypt::hash(password, bcrypt::DEFAULT_COST).unwrap();
        let user = User {
            id: user_id.clone(),
            username: "testuser".to_string(),
            email: "test@test.com".to_string(),
            password_hash,
            status: UserStatus::Active,
            attributes: serde_json::Value::Null,
            groups: vec![],
            policies: vec![],
        };
        user_repo.save(&user).await.unwrap();

        let use_case = LoginUseCase::new(&user_repo);
        let command = LoginCommand {
            username: "testuser".to_string(),
            password: password.to_string(),
        };
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(!response.token.is_empty());
    }

    #[tokio::test]
    async fn test_login_wrong_username() {
        let user_repo = MockUserRepository::new();
        let use_case = LoginUseCase::new(&user_repo);
        let command = LoginCommand {
            username: "wronguser".to_string(),
            password: "password123".to_string(),
        };
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        match result.err().unwrap() {
            crate::error::IamError::NotFound => (),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_login_wrong_password() {
        let user_repo = MockUserRepository::new();
        let user_id = UserId::new();
        let password = "password123";
        let password_hash = bcrypt::hash(password, bcrypt::DEFAULT_COST).unwrap();
        let user = User {
            id: user_id.clone(),
            username: "testuser".to_string(),
            email: "test@test.com".to_string(),
            password_hash,
            status: UserStatus::Active,
            attributes: serde_json::Value::Null,
            groups: vec![],
            policies: vec![],
        };
        user_repo.save(&user).await.unwrap();

        let use_case = LoginUseCase::new(&user_repo);
        let command = LoginCommand {
            username: "testuser".to_string(),
            password: "wrongpassword".to_string(),
        };
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        match result.err().unwrap() {
            crate::error::IamError::Unauthorized => (),
            _ => panic!("Expected Unauthorized error"),
        }
    }
}
