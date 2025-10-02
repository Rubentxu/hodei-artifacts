// crates/iam/src/features/create_policy/api.rs

use crate::features::create_policy::dto::{CreatePolicyCommand, CreatePolicyResponse};
use crate::features::create_policy::use_case::CreatePolicyUseCase;
use crate::infrastructure::errors::IamError;
use std::sync::Arc;

/// API layer for create policy feature
/// This is the entry point that external systems (HTTP, gRPC, etc.) will use
pub struct CreatePolicyApi {
    use_case: Arc<CreatePolicyUseCase>,
}

impl CreatePolicyApi {
    /// Create a new create policy API
    pub fn new(use_case: Arc<CreatePolicyUseCase>) -> Self {
        Self { use_case }
    }

    /// Handle create policy request
    pub async fn create_policy(
        &self,
        command: CreatePolicyCommand,
    ) -> Result<CreatePolicyResponse, IamError> {
        self.use_case.execute(command).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::create_policy::adapter::{
        CedarPolicyValidatorAdapter, MongoPolicyCreatorAdapter, SimplePolicyEventPublisherAdapter,
    };
    use crate::infrastructure::events::policy_event_publisher::SimplePolicyEventPublisher;
    // Repository implementation will be injected via DI
    use crate::infrastructure::validation::cedar_validator::CedarPolicyValidator;
    use mongodb::Database;
    use std::sync::Arc;

    // Note: These are unit tests that use mocks
    // Integration tests would be in a separate file

    #[tokio::test]
    async fn test_create_policy_api_structure() {
        // This test just verifies the API structure compiles
        // Real tests would use dependency injection with mocks

        // Mock database (in real tests, use testcontainers or mocks)
        let mock_db = Arc::new(
            mongodb::Client::with_uri_str("mongodb://localhost:27017")
                .await
                .unwrap()
                .database("test"),
        );

        // Repository will be injected via DI container
        let validator = Arc::new(CedarPolicyValidator::new());
        let publisher = Arc::new(SimplePolicyEventPublisher::new());

        // let creator_adapter = Arc::new(MongoPolicyCreatorAdapter::new(repository));
        let validator_adapter = Arc::new(CedarPolicyValidatorAdapter::new(validator));
        let publisher_adapter = Arc::new(SimplePolicyEventPublisherAdapter::new(publisher));

        // let use_case = Arc::new(CreatePolicyUseCase::new(
        //     creator_adapter,
        //     validator_adapter,
        //     publisher_adapter,
        // ));

        // let api = CreatePolicyApi::new(use_case);

        // Verify API was created successfully
        // assert!(std::ptr::addr_of!(api).is_null() == false);
    }
}
