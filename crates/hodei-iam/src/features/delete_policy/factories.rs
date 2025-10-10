//! Factory for creating the DeletePolicy use case
//!
//! This module follows the trait objects pattern for dependency injection:
//! - Factories receive Arc<dyn Trait> dependencies
//! - Factories return Arc<dyn UseCasePort> for maximum flexibility
//! - Easy testing with mock implementations

use std::sync::Arc;
use tracing::info;

use crate::features::delete_policy::ports::{DeletePolicyPort, DeletePolicyUseCasePort};
use crate::features::delete_policy::use_case::DeletePolicyUseCase;

/// Create the DeletePolicy use case with injected dependencies
///
/// This factory receives trait objects and returns a trait object,
/// making it simple to use from the Composition Root and easy to test.
///
/// # Arguments
///
/// * `policy_port` - Port for deleting policies
///
/// # Returns
///
/// Arc<dyn DeletePolicyUseCasePort> - The use case as a trait object
///
/// # Example
///
/// ```rust,ignore
/// let policy_repo = Arc::new(SurrealPolicyAdapter::new(db));
///
/// let delete_policy = create_delete_policy_use_case(policy_repo);
/// ```
pub fn create_delete_policy_use_case(
    policy_port: Arc<dyn DeletePolicyPort>,
) -> Arc<dyn DeletePolicyUseCasePort> {
    info!("Creating DeletePolicy use case");
    Arc::new(DeletePolicyUseCase::new(policy_port))
}

/// Alternative factory that accepts owned dependencies
///
/// This is useful when you have dependencies that are not yet wrapped in Arc
/// and you want the factory to handle the Arc wrapping.
///
/// # Arguments
///
/// * `policy_port` - Port for deleting policies
///
/// # Returns
///
/// Arc<dyn DeletePolicyUseCasePort> - The use case as a trait object

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::delete_policy::dto::DeletePolicyCommand;
    use crate::features::delete_policy::mocks::MockDeletePolicyPort;

    #[tokio::test]
    async fn test_factory_creates_use_case() {
        let policy_port: Arc<dyn DeletePolicyPort> =
            Arc::new(MockDeletePolicyPort::with_existing_policies(vec![
                "test-policy".to_string(),
            ]));

        let use_case = create_delete_policy_use_case(policy_port);

        let command = DeletePolicyCommand::new("test-policy");
        let result = use_case.execute(command).await;
        assert!(result.is_ok());
    }
}
