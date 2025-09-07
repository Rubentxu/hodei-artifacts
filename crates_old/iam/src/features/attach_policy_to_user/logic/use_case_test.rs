#[cfg(test)]
mod tests {
    use crate::features::attach_policy_to_user::logic::use_case::AttachPolicyToUserUseCase;
    use crate::features::attach_policy_to_user::command::AttachPolicyToUserCommand;
    use crate::mocks::user_repository::MockUserRepository;
    use crate::mocks::policy_repository::MockPolicyRepository;
    use crate::application::ports::{UserRepository, PolicyRepository};
    use crate::domain::user::{User, UserStatus};
    use crate::domain::policy::{Policy, PolicyStatus};
    use shared::UserId;
    use cedar_policy::PolicyId;

    #[tokio::test]
    async fn test_attach_policy_to_user_success() {
        let user_repo = MockUserRepository::new();
        let policy_repo = MockPolicyRepository::new();
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
            policies: vec![],
        };
        let policy = Policy {
            id: policy_id.clone(),
            name: "test_policy".to_string(),
            description: None,
            version: 1,
            content: "permit(principal, action, resource);".to_string(),
            status: PolicyStatus::Active,
        };
        user_repo.save(&user).await.unwrap();
        policy_repo.save(&policy).await.unwrap();

        let use_case = AttachPolicyToUserUseCase::new(&user_repo, &policy_repo);
        let command = AttachPolicyToUserCommand { user_id: user_id.clone(), policy_id: policy_id.clone() };
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let updated_user = user_repo.find_by_id(&user_id).await.unwrap().unwrap();
        assert_eq!(updated_user.policies, vec![policy_id]);
    }

    #[tokio::test]
    async fn test_attach_policy_to_user_user_not_found() {
        let user_repo = MockUserRepository::new();
        let policy_repo = MockPolicyRepository::new();
        let use_case = AttachPolicyToUserUseCase::new(&user_repo, &policy_repo);
        let command = AttachPolicyToUserCommand { user_id: UserId::new(), policy_id: PolicyId::new("test_policy") };
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        match result.err().unwrap() {
            crate::error::IamError::NotFound => (),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_attach_policy_to_user_policy_not_found() {
        let user_repo = MockUserRepository::new();
        let policy_repo = MockPolicyRepository::new();
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

        let use_case = AttachPolicyToUserUseCase::new(&user_repo, &policy_repo);
        let command = AttachPolicyToUserCommand { user_id: user_id.clone(), policy_id: PolicyId::new("test_policy") };
        let result = use_case.execute(command).await;

        assert!(result.is_err());
        match result.err().unwrap() {
            crate::error::IamError::NotFound => (),
            _ => panic!("Expected NotFound error"),
        }
    }
}
