use std::sync::Arc;

use super::{
    adapter::{SingleArtifactProcessor, NoopTransactionManager},
    ports::{BatchArtifactProcessor, BatchTransactionManager},
    use_case::BatchUploadUseCase,
    api::BatchUploadEndpoint,
};
use crate::features::upload_artifact::use_case::UploadArtifactUseCase;

/// The Dependency Injection container for the Batch Upload feature.
pub struct BatchUploadDIContainer {
    pub endpoint: Arc<BatchUploadEndpoint>,
    pub use_case: Arc<BatchUploadUseCase>,
}

impl BatchUploadDIContainer {
    /// Wires up the dependencies for this feature.
    pub fn new(
        artifact_processor: Arc<dyn BatchArtifactProcessor + Send + Sync>,
        transaction_manager: Arc<dyn BatchTransactionManager + Send + Sync>,
        batch_timeout_seconds: u64,
    ) -> Self {
        let use_case = Arc::new(BatchUploadUseCase::new(
            artifact_processor,
            transaction_manager,
            batch_timeout_seconds,
        ));
        let endpoint = Arc::new(BatchUploadEndpoint::new(use_case.clone()));

        Self { endpoint, use_case }
    }

    /// Convenience function for wiring up production dependencies.
    pub fn for_production(
        upload_use_case: Arc<UploadArtifactUseCase>,
    ) -> Self {
        let artifact_processor: Arc<dyn BatchArtifactProcessor> = 
            Arc::new(SingleArtifactProcessor::new(upload_use_case));
        
        let transaction_manager: Arc<dyn BatchTransactionManager> = 
            Arc::new(NoopTransactionManager);

        Self::new(artifact_processor, transaction_manager, 300) // 5 minute timeout
    }

    /// Convenience function for wiring up mock dependencies for testing.
    #[cfg(test)]
    pub fn for_testing() -> (
        Self, 
        Arc<super::adapter::test::MockBatchArtifactProcessor>,
        Arc<super::adapter::test::MockTransactionManager>
    ) {
        use super::adapter::test::{MockBatchArtifactProcessor, MockTransactionManager};

        let artifact_processor = Arc::new(MockBatchArtifactProcessor::new(false));
        let transaction_manager = Arc::new(MockTransactionManager::new(false));

        let container = Self::new(
            artifact_processor.clone(),
            transaction_manager.clone(),
            30, // 30 second timeout for tests
        );

        (container, artifact_processor, transaction_manager)
    }

    /// Convenience function for wiring up mock dependencies with transaction support.
    #[cfg(test)]
    pub fn for_testing_with_transactions() -> (
        Self, 
        Arc<super::adapter::test::MockBatchArtifactProcessor>,
        Arc<super::adapter::test::MockTransactionManager>
    ) {
        use super::adapter::test::{MockBatchArtifactProcessor, MockTransactionManager};

        let artifact_processor = Arc::new(MockBatchArtifactProcessor::new(false));
        let transaction_manager = Arc::new(MockTransactionManager::new(true));

        let container = Self::new(
            artifact_processor.clone(),
            transaction_manager.clone(),
            30, // 30 second timeout for tests
        );

        (container, artifact_processor, transaction_manager)
    }
}