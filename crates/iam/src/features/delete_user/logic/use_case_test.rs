#[cfg(test)]
mod tests {
    use crate::features::delete_user::logic::use_case::DeleteUserUseCase;
    use crate::features::delete_user::command::DeleteUserCommand;
    use crate::mocks::user_repository::MockUserRepository;
    use crate::application::ports::UserRepository;
    use crate::domain::user::{User, UserStatus};
    use shared::UserId;

    #[tokio::test]
    async fn test_delete_user_success() {
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

        let use_case = DeleteUserUseCase::new(&user_repo);
        let command = DeleteUserCommand { id: user_id.clone() };
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let found_user = user_repo.find_by_id(&user_id).await.unwrap();
        assert!(found_user.is_none());
    }
}
