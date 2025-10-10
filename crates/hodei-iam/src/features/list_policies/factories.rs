//! Factory for creating the ListPolicies use case
//!
//! This module follows the Shaku pattern for dependency injection:
//! - Factories receive Arc<dyn Trait> dependencies
//! - Factories return Arc<dyn UseCasePort> for maximum flexibility
//! - Constructor injection pattern for easy testing

use std::sync::Arc;
use tracing::info;

use crate::features::list_policies::ports::{ListPoliciesUseCasePort, PolicyLister};
use crate::features::list_policies::use_case::ListPoliciesUseCase;

/// Create the ListPolicies use case with injected dependencies
///
/// This factory receives trait objects and returns a trait object,
/// following the Shaku pattern for dependency injection.
///
/// # Arguments
///
/// * `policy_lister` - Port for listing policies
///
/// # Returns
///
/// Arc<dyn ListPoliciesUseCasePort> - The use case as a trait object
///
/// # Example
///
/// ```rust,ignore
/// let policy_lister = Arc::new(SurrealPolicyAdapter::new(db));
///
/// let list_policies = create_list_policies_use_case(policy_lister);
/// ```
pub fn create_list_policies_use_case(
    policy_lister: Arc<dyn PolicyLister>,
) -> Arc<dyn ListPoliciesUseCasePort> {
    info!("Creating ListPolicies use case");
    Arc::new(ListPoliciesUseCase::new(policy_lister))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::list_policies::dto::ListPoliciesQuery;
    use crate::features::list_policies::mocks::MockPolicyLister;

    #[tokio::test]
    async fn test_factory_creates_use_case() {
        let policy_lister: Arc<dyn PolicyLister> = Arc::new(MockPolicyLister::new());

        let use_case = create_list_policies_use_case(policy_lister);

        let query = ListPoliciesQuery::default();
        let result = use_case.execute(query).await;
        assert!(result.is_ok());
    }
}
