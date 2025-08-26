#[cfg(test)]
mod tests {
    use crate::features::list_users::logic::use_case::ListUsersUseCase;
    use crate::features::list_users::query::ListUsersQuery;
    use crate::mocks::user_repository::MockUserRepository;
    use crate::application::ports::UserRepository;
    use crate::domain::user::{User, UserStatus};
    use shared::UserId;

    #[tokio::test]
    async fn test_list_users_success() {
        let user_repo = MockUserRepository::new();
        let user1 = User {
            id: UserId::new(),
            username: "testuser1".to_string(),
            email: "test1@test.com".to_string(),
            password_hash: "testhash1".to_string(),
            status: UserStatus::Active,
            attributes: serde_json::Value::Null,
            groups: vec![],
            policies: vec![],
        };
        let user2 = User {
            id: UserId::new(),
            username: "testuser2".to_string(),
            email: "test2@test.com".to_string(),
            password_hash: "testhash2".to_string(),
            status: UserStatus::Active,
            attributes: serde_json::Value::Null,
            groups: vec![],
            policies: vec![],
        };
        user_repo.save(&user1).await.unwrap();
        user_repo.save(&user2).await.unwrap();

        let use_case = ListUsersUseCase::new(&user_repo);
        let result = use_case.execute().await;

        assert!(result.is_ok());
        let users = result.unwrap();
        assert_eq!(users.len(), 2);
    }

    #[tokio::test]
    async fn test_list_users_empty() {
        let user_repo = MockUserRepository::new();
        let use_case = ListUsersUseCase::new(&user_repo);
        let result = use_case.execute().await;

        assert!(result.is_ok());
        let users = result.unwrap();
        assert_eq!(users.len(), 0);
    }
}
