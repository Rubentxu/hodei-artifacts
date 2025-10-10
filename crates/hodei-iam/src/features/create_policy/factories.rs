//! Factory for creating the CreatePolicy use case
//!
//! This module follows a simple pattern for dependency injection:
//! - Factories receive Arc<dyn Trait> dependencies
//! - Factories return Arc<dyn Port> for the use case
//! - No complex generics, just trait objects

use std::sync::Arc;
use tracing::info;

use crate::features::create_policy::ports::{
    CreatePolicyPort, CreatePolicyUseCasePort, PolicyValidator,
};
use crate::features::create_policy::use_case::CreatePolicyUseCase;

/// Create the CreatePolicy use case with injected dependencies
///
/// This factory accepts trait objects and returns a trait object,
/// making it simple to use from the Composition Root.
///
/// # Arguments
///
/// * `policy_port` - Repository for persisting policies
/// * `validator` - Validator for Cedar policy syntax
///
/// # Returns
///
/// Arc<dyn CreatePolicyUseCasePort> - The use case as a trait object
///
/// # Example
///
/// ```rust,ignore
/// let policy_repo = Arc::new(SurrealPolicyAdapter::new(db));
/// let validator = hodei_policies_validate_port;
///
/// let create_policy = create_policy_use_case(
///     policy_repo,
///     validator,
/// );
/// ```
pub fn create_policy_use_case(
    policy_port: Arc<dyn CreatePolicyPort>,
    validator: Arc<dyn PolicyValidator>,
) -> Arc<dyn CreatePolicyUseCasePort> {
    info!("Creating CreatePolicy use case");
    Arc::new(CreatePolicyUseCase::new(policy_port, validator))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy::dto::CreatePolicyCommand;
    use crate::features::create_policy::mocks::{MockCreatePolicyPort, MockPolicyValidator};

    #[tokio::test]
    async fn test_factory_creates_use_case() {
        let policy_port: Arc<dyn CreatePolicyPort> = Arc::new(MockCreatePolicyPort::new());
        let validator: Arc<dyn PolicyValidator> = Arc::new(MockPolicyValidator::new());

        let use_case = create_policy_use_case(policy_port, validator);

        let command = CreatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: "permit(principal, action, resource);".to_string(),
            description: Some("Test".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }
}
