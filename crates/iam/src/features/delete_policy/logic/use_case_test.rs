#[cfg(test)]
mod tests {
    use crate::features::delete_policy::logic::use_case::DeletePolicyUseCase;
    use crate::features::delete_policy::command::DeletePolicyCommand;
    use crate::mocks::policy_repository::MockPolicyRepository;
    use crate::application::ports::PolicyRepository;
    use crate::domain::policy::{Policy, PolicyStatus};
    use cedar_policy::PolicyId;

    #[tokio::test]
    async fn test_delete_policy_success() {
        let policy_repo = MockPolicyRepository::new();
        let policy_id = PolicyId::new("test_policy");
        let policy = Policy {
            id: policy_id.clone(),
            name: "test_policy".to_string(),
            description: None,
            version: 1,
            content: "permit(principal, action, resource);".to_string(),
            status: PolicyStatus::Active,
        };
        policy_repo.save(&policy).await.unwrap();

        let use_case = DeletePolicyUseCase::new(&policy_repo);
        let command = DeletePolicyCommand { id: policy_id.clone() };
        let result = use_case.execute(command).await;

        assert!(result.is_ok());
        let found_policy = policy_repo.find_by_id(&policy_id).await.unwrap();
        assert!(found_policy.is_none());
    }
}
