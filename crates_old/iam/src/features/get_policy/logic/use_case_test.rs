#[cfg(test)]
mod tests {
    use crate::features::get_policy::logic::use_case::GetPolicyUseCase;
    use crate::features::get_policy::query::GetPolicyQuery;
    use crate::mocks::policy_repository::MockPolicyRepository;
    use crate::application::ports::PolicyRepository;
    use crate::domain::policy::{Policy, PolicyStatus};
    use cedar_policy::PolicyId;

    #[tokio::test]
    async fn test_get_policy_success() {
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

        let use_case = GetPolicyUseCase::new(&policy_repo);
        let query = GetPolicyQuery { id: policy_id };
        let result = use_case.execute(query).await;

        assert!(result.is_ok());
        let found_policy = result.unwrap();
        assert_eq!(found_policy, policy);
    }

    #[tokio::test]
    async fn test_get_policy_not_found() {
        let policy_repo = MockPolicyRepository::new();
        let use_case = GetPolicyUseCase::new(&policy_repo);
        let query = GetPolicyQuery { id: PolicyId::new("test_policy") };
        let result = use_case.execute(query).await;

        assert!(result.is_err());
        match result.err().unwrap() {
            crate::error::IamError::NotFound => (),
            _ => panic!("Expected NotFound error"),
        }
    }
}
