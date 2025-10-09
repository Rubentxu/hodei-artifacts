//! Factory for creating the CreatePolicy use case
//!
//! This module follows the Java Config pattern for dependency injection:
//! - Factories receive already-constructed dependencies
//! - Factories return trait objects (Arc<dyn Port>) for maximum flexibility
//! - No internal construction of dependencies (Composition Root does that)

use std::sync::Arc;
use tracing::info;

use crate::features::create_policy::ports::{
    CreatePolicyPort, CreatePolicyUseCasePort, PolicyValidator,
};
use crate::features::create_policy::use_case::CreatePolicyUseCase;

/// Create the CreatePolicy use case with injected dependencies
///
/// This factory follows the Java Config pattern:
/// - Receives already-constructed dependencies
/// - Returns Arc<dyn Port> for the use case
/// - No knowledge of concrete types
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
/// let create_policy = create_create_policy_use_case(
///     policy_repo,
///     validator,
/// );
/// ```
pub fn create_create_policy_use_case<P, V>(
    policy_port: Arc<P>,
    validator: Arc<V>,
) -> Arc<dyn CreatePolicyUseCasePort>
where
    P: CreatePolicyPort + 'static,
    V: PolicyValidator + 'static,
{
    info!("Creating CreatePolicy use case");
    Arc::new(CreatePolicyUseCase::new(policy_port, validator))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy::dto::CreatePolicyCommand;
    use crate::features::create_policy::mocks::{MockCreatePolicyPort, MockPolicyValidator};

    #[tokio::test]
    async fn factory_creates_use_case_successfully() {
        let port = Arc::new(MockCreatePolicyPort::new());
        let validator = Arc::new(MockPolicyValidator::new());
        let use_case = create_create_policy_use_case(port.clone(), validator);

        let cmd = CreatePolicyCommand {
            policy_id: "test-policy".into(),
            policy_content: "permit(principal, action, resource);".into(),
            description: None,
        };

        let result = use_case.execute(cmd).await;
        assert!(result.is_ok());
        assert_eq!(port.get_created_count(), 1);
    }
}
