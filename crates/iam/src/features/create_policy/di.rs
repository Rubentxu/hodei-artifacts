// crates/iam/src/features/create_policy/di.rs

use crate::features::create_policy::adapter::{
    CedarPolicyValidatorAdapter, MongoPolicyCreatorAdapter, SimplePolicyEventPublisherAdapter,
};
use crate::features::create_policy::api::CreatePolicyApi;
use crate::features::create_policy::use_case::CreatePolicyUseCase;
use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
// Repository implementations are now embedded in adapters
use crate::infrastructure::validation::cedar_validator::CedarPolicyValidator;
use mongodb::Database;
use std::sync::Arc;

/// Dependency injection configuration for create_policy feature
pub struct CreatePolicyDI;

impl CreatePolicyDI {
    /// Wire up all dependencies and return the API
    pub fn wire_dependencies(database: Arc<Database>) -> CreatePolicyApi {
        // Infrastructure layer
        let validator = Arc::new(CedarPolicyValidator::new());
        let publisher = Arc::new(SimplePolicyEventPublisher::new());

        // Adapters (interface implementations)
        let creator_adapter = Arc::new(MongoPolicyCreatorAdapter::new(database));
        let validator_adapter = Arc::new(CedarPolicyValidatorAdapter::new(validator));
        let publisher_adapter = Arc::new(SimplePolicyEventPublisherAdapter::new(publisher));

        // Use case (business logic)
        let use_case = Arc::new(CreatePolicyUseCase::new(
            creator_adapter,
            validator_adapter,
            publisher_adapter,
        ));

        // API layer
        CreatePolicyApi::new(use_case)
    }

    /// Alternative wiring for testing with mocks
    pub fn wire_with_mocks(
        creator: Arc<dyn crate::features::create_policy::ports::PolicyCreator>,
        validator: Arc<dyn crate::features::create_policy::ports::PolicyValidator>,
        publisher: Arc<dyn crate::features::create_policy::ports::PolicyEventPublisher>,
    ) -> CreatePolicyApi {
        let use_case = Arc::new(CreatePolicyUseCase::new(creator, validator, publisher));
        CreatePolicyApi::new(use_case)
    }
}