// crates/iam/src/features/list_policies/di.rs

use crate::features::list_policies::adapter::ListPoliciesAdapter;
use crate::features::list_policies::api::ListPoliciesApi;
use crate::features::list_policies::use_case::ListPoliciesUseCase;
use mongodb::Database;
use std::sync::Arc;

/// Dependency injection configuration for list_policies feature
pub struct ListPoliciesDI;

impl ListPoliciesDI {
    /// Wire up all dependencies and return the API
    pub fn wire_dependencies(database: Arc<Database>) -> ListPoliciesApi {
        // Adapters (interface implementations)
        let lister_adapter = Arc::new(ListPoliciesAdapter::new(database));

        // Use case (business logic)
        let use_case = Arc::new(ListPoliciesUseCase::new(lister_adapter));

        // API layer
        ListPoliciesApi::new(use_case)
    }

    /// Alternative wiring for testing with mocks
    pub fn wire_with_mocks(
        lister: Arc<dyn crate::features::list_policies::ports::PolicyLister>,
    ) -> ListPoliciesApi {
        let use_case = Arc::new(ListPoliciesUseCase::new(lister));
        ListPoliciesApi::new(use_case)
    }
}
