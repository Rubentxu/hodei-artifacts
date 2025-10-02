// crates/iam/src/features/update_policy/di.rs

use crate::features::update_policy::adapter::UpdatePolicyAdapter;
use crate::features::update_policy::api::UpdatePolicyApi;
use crate::features::update_policy::use_case::UpdatePolicyUseCase;
use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
use crate::infrastructure::validation::cedar_validator::CedarPolicyValidator;
use mongodb::Database;
use std::sync::Arc;

/// Dependency injection configuration for update_policy feature
pub struct UpdatePolicyDI;

impl UpdatePolicyDI {
    /// Wire up all dependencies and return the API
    pub fn wire_dependencies(database: Arc<Database>) -> UpdatePolicyApi {
        // Infrastructure layer
        let validator = Arc::new(CedarPolicyValidator::new());
        let publisher = Arc::new(SimplePolicyEventPublisher::new());

        // Adapters (interface implementations)
        let adapter = Arc::new(UpdatePolicyAdapter::new(database, validator, publisher));

        // Use case (business logic)
        let use_case = Arc::new(UpdatePolicyUseCase::new(
            adapter.clone(),
            adapter.clone(),
            adapter,
        ));

        // API layer
        UpdatePolicyApi::new(use_case)
    }

    /// Alternative wiring for testing with mocks
    pub fn wire_with_mocks(
        updater: Arc<dyn crate::features::update_policy::ports::PolicyUpdater>,
        validator: Arc<dyn crate::features::update_policy::ports::PolicyUpdateValidator>,
        publisher: Arc<dyn crate::features::update_policy::ports::PolicyUpdateEventPublisher>,
    ) -> UpdatePolicyApi {
        let use_case = Arc::new(UpdatePolicyUseCase::new(updater, validator, publisher));
        UpdatePolicyApi::new(use_case)
    }
}
