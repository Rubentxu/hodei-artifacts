//! Factory for creating the GetPolicy use case
//!
//! This module follows a simple pattern for dependency injection:
//! - Factories receive Arc<dyn Trait> dependencies
//! - Factories return Arc<dyn Port> for the use case
//! - No complex generics, just trait objects

use std::sync::Arc;
use tracing::info;

use crate::features::get_policy::dto::PolicyView;
use crate::features::get_policy::ports::{GetPolicyUseCasePort, PolicyReader};
use crate::features::get_policy::use_case::GetPolicyUseCase;

/// Create the GetPolicy use case with injected dependencies
///
/// This factory receives trait objects and returns a trait object,
/// making it simple to use from the Composition Root and easy to test.
///
/// # Arguments
///
/// * `policy_reader` - Port for reading policies
///
/// # Returns
///
/// Arc<dyn GetPolicyUseCasePort> - The use case as a trait object
///
/// # Example
///
/// ```rust,ignore
/// let policy_reader = Arc::new(SurrealPolicyAdapter::new(db));
///
/// let get_policy = create_get_policy_use_case(policy_reader);
/// ```
pub fn create_get_policy_use_case(
    policy_reader: Arc<dyn PolicyReader>,
) -> Arc<dyn GetPolicyUseCasePort> {
    info!("Creating GetPolicy use case");
    Arc::new(GetPolicyUseCase::new(policy_reader))
}

/// Alternative factory that accepts owned dependencies
///
/// This is useful when you have dependencies that are not yet wrapped in Arc
/// and you want the factory to handle the Arc wrapping.
///
/// # Arguments
///
/// * `policy_reader` - Port for reading policies
///
/// # Returns
///
/// Arc<dyn GetPolicyUseCasePort> - The use case as a trait object
pub fn create_get_policy_use_case_from_owned<P>(policy_reader: P) -> Arc<dyn GetPolicyUseCasePort>
where
    P: PolicyReader + 'static,
{
    info!("Creating GetPolicy use case from owned dependencies");
    Arc::new(GetPolicyUseCase::new(Arc::new(policy_reader)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::get_policy::dto::GetPolicyQuery;
    use crate::features::get_policy::mocks::MockPolicyReader;

    #[tokio::test]
    async fn test_factory_creates_use_case() {
        let policy = PolicyView {
            hrn: kernel::Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "Policy".to_string(),
                "test-policy".to_string(),
            ),
            name: "test-policy".to_string(),
            content: "permit(principal, action, resource);".to_string(),
            description: None,
        };
        let policy_reader: Arc<dyn PolicyReader> = Arc::new(MockPolicyReader::with_policy(policy));

        let use_case = create_get_policy_use_case(policy_reader);

        let query = GetPolicyQuery {
            policy_hrn: kernel::Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "Policy".to_string(),
                "test-policy".to_string(),
            ),
        };
        let result = use_case.execute(query).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_factory_from_owned_works() {
        let policy = PolicyView {
            hrn: kernel::Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "Policy".to_string(),
                "owned".to_string(),
            ),
            name: "owned-policy".to_string(),
            content: "permit(principal, action, resource);".to_string(),
            description: None,
        };
        let policy_reader = MockPolicyReader::with_policy(policy);

        let use_case = create_get_policy_use_case_from_owned(policy_reader);

        let query = GetPolicyQuery {
            policy_hrn: kernel::Hrn::new(
                "hodei".to_string(),
                "iam".to_string(),
                "default".to_string(),
                "Policy".to_string(),
                "owned".to_string(),
            ),
        };
        let result = use_case.execute(query).await;
        assert!(result.is_ok());
    }
}
