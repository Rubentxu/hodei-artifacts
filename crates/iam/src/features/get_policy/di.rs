// crates/iam/src/features/get_policy/di.rs

use crate::features::get_policy::adapter::MongoPolicyReaderAdapter;
use crate::features::get_policy::api::GetPolicyApi;
use crate::features::get_policy::use_case::GetPolicyUseCase;
// Repository implementations are now embedded in adapters
use mongodb::Database;
use std::sync::Arc;

/// Dependency injection configuration for get_policy feature
pub struct GetPolicyDI;

impl GetPolicyDI {
    /// Wire up all dependencies and return the API
    pub fn wire_dependencies(database: Arc<Database>) -> GetPolicyApi {
        // Adapters (interface implementations)
        let reader_adapter = Arc::new(MongoPolicyReaderAdapter::new(database));

        // Use case (business logic)
        let use_case = Arc::new(GetPolicyUseCase::new(reader_adapter));

        // API layer
        GetPolicyApi::new(use_case)
    }

    /// Alternative wiring for testing with mocks
    pub fn wire_with_mocks(
        reader: Arc<dyn crate::features::get_policy::ports::PolicyReader>,
    ) -> GetPolicyApi {
        let use_case = Arc::new(GetPolicyUseCase::new(reader));
        GetPolicyApi::new(use_case)
    }
}
