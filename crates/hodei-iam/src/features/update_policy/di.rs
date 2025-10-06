//! Dependency Injection helpers for Update Policy feature

use super::adapter::InMemoryUpdatePolicyAdapter;
use super::use_case::UpdatePolicyUseCase;
use crate::features::create_policy_new::CedarPolicyValidator;
use std::sync::Arc;

/// Create an UpdatePolicyUseCase with in-memory adapter
pub fn make_update_policy_uc() -> UpdatePolicyUseCase<CedarPolicyValidator, InMemoryUpdatePolicyAdapter> {
    let validator = Arc::new(CedarPolicyValidator::new());
    let adapter = Arc::new(InMemoryUpdatePolicyAdapter::new());
    UpdatePolicyUseCase::new(validator, adapter)
}

/// Create an UpdatePolicyUseCase with custom validator and adapter
pub fn make_update_policy_uc_with<V, P>(
    validator: Arc<V>,
    adapter: Arc<P>,
) -> UpdatePolicyUseCase<V, P>
where
    V: super::ports::PolicyValidator,
    P: super::ports::UpdatePolicyPort,
{
    UpdatePolicyUseCase::new(validator, adapter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::update_policy::dto::UpdatePolicyCommand;

    #[tokio::test]
    async fn test_make_update_policy_uc() {
        let use_case = make_update_policy_uc();

        // Add a test policy to the adapter
        let adapter = Arc::new(InMemoryUpdatePolicyAdapter::new());
        adapter.add_policy(
            "test-policy".to_string(),
            "permit(principal, action, resource);".to_string(),
            Some("Test".to_string()),
        );

        let use_case_with_data = UpdatePolicyUseCase::new(
            Arc::new(CedarPolicyValidator::new()),
            adapter,
        );

        let command = UpdatePolicyCommand::update_description("test-policy", "Updated");
        let result = use_case_with_data.execute(command).await;

        assert!(result.is_ok());
    }
}
