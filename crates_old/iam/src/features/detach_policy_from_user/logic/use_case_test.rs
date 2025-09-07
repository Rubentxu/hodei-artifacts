#[cfg(test)]
mod tests {
    use crate::features::detach_policy_from_user::logic::use_case::DetachPolicyFromUserUseCase;
    use crate::features::detach_policy_from_user::command::DetachPolicyFromUserCommand;
    use crate::mocks::user_repository::MockUserRepository;
    use crate::application::ports::UserRepository;
    use crate::domain::user::{User, UserStatus};
    use shared::UserId;
    use cedar_policy::PolicyId;

    #[tokio::test]
    async fn test_detach_policy_from_user_success() {
        let user_repo = MockUserRepository::new();
        let user_id = UserId::new();
        let policy_id = PolicyId::new("test_policy");
        let user = User {
            id: user_id.clone(),
            username: "testuser".to_string(),
            email: "test@test.com".to_string(),
            password_hash: "testhash".to_string(),
            status: UserStatus::Active,
            attributes: serde_json::Value::Null,
            groups: vec![],
            policies: vec![policy_id.clone()],
        };
        user_repo.save(&user).await.unwrap();

        let use_case = DetachPolicyFromUserUseCase::new(&user_repo);
        let command = DetachPolicyFromUserCommand { user_id: user_id.clone(), policy_id: policy_id.clone() };
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let updated_user = user_repo.find_by_id(&user_id).await.unwrap().unwrap();
        assert!(updated_user.policies.is_empty());
    }

    #[tokio::test]
    async fn test_detach_policy_from_user_user_not_found() {
        let user_repo = MockUserRepository::new();
        let use_case = DetachPolicyFromUserUseCase::new(&user_repo);
        let command = DetachPolicyFromUserCommand { user_id: UserId::new(), policy_id: PolicyId::new("test_policy") };
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        match result.err().unwrap() {
            crate::error::IamError::NotFound => (),
            _ => panic!("Expected NotFound error"),
        }
    }
}
