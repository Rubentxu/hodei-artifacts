// crates/iam/src/features/delete_policy/di.rs

use crate::features::delete_policy::adapter::DeletePolicyAdapter;
use crate::features::delete_policy::api::DeletePolicyApi;
use crate::features::delete_policy::use_case::DeletePolicyUseCase;
use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
use mongodb::Database;
use std::sync::Arc;

/// Dependency injection configuration for delete_policy feature
pub struct DeletePolicyDI;

impl DeletePolicyDI {
    /// Wire up all dependencies and return the API
    pub fn wire_dependencies(database: Arc<Database>) -> DeletePolicyApi {
        // Infrastructure layer
        let publisher = Arc::new(SimplePolicyEventPublisher::new());

        // Adapters (interface implementations)
        let adapter = Arc::new(DeletePolicyAdapter::new(database, publisher));

        // Use case (business logic)
        let use_case = Arc::new(DeletePolicyUseCase::new(adapter.clone(), adapter));

        // API layer
        DeletePolicyApi::new(use_case)
    }

    /// Alternative wiring for testing with mocks
    pub fn wire_with_mocks(
        deleter: Arc<dyn crate::features::delete_policy::ports::PolicyDeleter>,
        publisher: Arc<dyn crate::features::delete_policy::ports::PolicyDeleteEventPublisher>,
    ) -> DeletePolicyApi {
        let use_case = Arc::new(DeletePolicyUseCase::new(deleter, publisher));
        DeletePolicyApi::new(use_case)
    }
}
