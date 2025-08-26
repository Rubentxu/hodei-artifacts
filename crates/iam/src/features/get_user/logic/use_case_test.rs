#[cfg(test)]
mod tests {
    use crate::features::get_user::logic::use_case::GetUserUseCase;
    use crate::features::get_user::query::GetUserQuery;
    use crate::mocks::user_repository::MockUserRepository;
    use crate::application::ports::UserRepository;
    use crate::domain::user::{User, UserStatus};
    use shared::UserId;

    #[tokio::test]
    async fn test_get_user_success() {
        let user_repo = MockUserRepository::new();
        let user_id = UserId::new();
        let user = User {
            id: user_id.clone(),
            username: "testuser".to_string(),
            email: "test@test.com".to_string(),
            password_hash: "testhash".to_string(),
            status: UserStatus::Active,
            attributes: serde_json::Value::Null,
            groups: vec![],
            policies: vec![],
        };
        user_repo.save(&user).await.unwrap();

        let use_case = GetUserUseCase::new(&user_repo);
        let query = GetUserQuery { id: user_id };
        let result = use_case.execute(query).await;

        assert!(result.is_ok());
        let found_user = result.unwrap();
        assert_eq!(found_user, user);
    }

    #[tokio::test]
    async fn test_get_user_not_found() {
        let user_repo = MockUserRepository::new();
        let use_case = GetUserUseCase::new(&user_repo);
        let query = GetUserQuery { id: UserId::new() };
        let result = use_case.execute(query).await;

        assert!(result.is_err());
        match result.err().unwrap() {
            crate::error::IamError::NotFound => (),
            _ => panic!("Expected NotFound error"),
        }
    }
}
