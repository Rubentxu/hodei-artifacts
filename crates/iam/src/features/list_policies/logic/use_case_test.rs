#[cfg(test)]
mod tests {
    use crate::features::list_policies::logic::use_case::ListPoliciesUseCase;
    use crate::mocks::policy_repository::MockPolicyRepository;
    use crate::application::ports::PolicyRepository;
    use crate::domain::policy::{Policy, PolicyStatus};
    use cedar_policy::PolicyId;

    #[tokio::test]
    async fn test_list_policies_success() {
        let policy_repo = MockPolicyRepository::new();
        let policy1 = Policy {
            id: PolicyId::new("test_policy1"),
            name: "test_policy1".to_string(),
            description: None,
            version: 1,
            content: "permit(principal, action, resource);".to_string(),
            status: PolicyStatus::Active,
        };
        let policy2 = Policy {
            id: PolicyId::new("test_policy2"),
            name: "test_policy2".to_string(),
            description: None,
            version: 1,
            content: "permit(principal, action, resource);".to_string(),
            status: PolicyStatus::Active,
        };
        policy_repo.save(&policy1).await.unwrap();
        policy_repo.save(&policy2).await.unwrap();

        let use_case = ListPoliciesUseCase::new(&policy_repo);
        let result = use_case.execute().await;

        assert!(result.is_ok());
        let policies = result.unwrap();
        assert_eq!(policies.len(), 2);
    }

    #[tokio::test]
    async fn test_list_policies_empty() {
        let policy_repo = MockPolicyRepository::new();
        let use_case = ListPoliciesUseCase::new(&policy_repo);
        let result = use_case.execute().await;

        assert!(result.is_ok());
        let policies = result.unwrap();
        assert_eq!(policies.len(), 0);
    }
}
