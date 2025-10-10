//! Factory for creating the UpdatePolicy use case
//!
//! This module follows the trait objects pattern for dependency injection:
//! - Factories receive Arc<dyn Trait> dependencies
//! - Factories return Arc<dyn UseCasePort> for maximum flexibility
//! - Easy testing with mock implementations

use std::sync::Arc;
use tracing::info;

use super::ports::{PolicyValidator, UpdatePolicyPort};
use super::use_case::UpdatePolicyUseCase;

/// Create the UpdatePolicy use case with injected dependencies
///
/// This factory receives trait objects and returns the use case,
/// making it simple to use from the Composition Root and easy to test.
///
/// # Arguments
///
/// * `validator` - Port for validating Cedar policy syntax
/// * `policy_port` - Port for updating policies in storage
///
/// # Returns
///
/// UpdatePolicyUseCase - The use case instance
///
/// # Example
///
/// ```rust,ignore
/// let validator = Arc::new(CedarPolicyValidator::new(schema_storage));
/// let policy_port = Arc::new(SurrealPolicyAdapter::new(db));
///
/// let update_policy = update_policy_use_case(
///     validator,
///     policy_port,
/// );
/// ```
pub fn update_policy_use_case(
    validator: Arc<dyn PolicyValidator>,
    policy_port: Arc<dyn UpdatePolicyPort>,
) -> UpdatePolicyUseCase {
    info!("Creating UpdatePolicy use case");
    UpdatePolicyUseCase::new(validator, policy_port)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::update_policy::dto::UpdatePolicyCommand;
    use crate::features::update_policy::mocks::{MockPolicyValidator, MockUpdatePolicyPort};

    #[tokio::test]
    async fn test_factory_creates_use_case() {
        let validator: Arc<dyn PolicyValidator> = Arc::new(MockPolicyValidator::new());
        let policy_port: Arc<dyn UpdatePolicyPort> = Arc::new(MockUpdatePolicyPort::new());

        let use_case = update_policy_use_case(validator, policy_port);

        let command = UpdatePolicyCommand {
            policy_id: "test-policy".to_string(),
            policy_content: Some("permit(principal, action, resource);".to_string()),
            description: Some("Test description".to_string()),
        };

        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }
}
