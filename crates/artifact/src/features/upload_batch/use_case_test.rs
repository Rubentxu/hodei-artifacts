use std::sync::Arc;
use tokio::time::{sleep, Duration};

use super::{
    use_case::BatchUploadUseCase,
    dto::{BatchUploadArtifactCommand, BatchUploadStatus},
    ports::{BatchArtifactProcessor, BatchTransactionManager},
    error::BatchUploadError,
    adapter::test::{MockBatchArtifactProcessor, MockTransactionManager},
};

#[tokio::test]
async fn test_execute_batch_success() {
    let artifact_processor = Arc::new(MockBatchArtifactProcessor::new(false));
    let transaction_manager = Arc::new(MockTransactionManager::new(false));
    
    let use_case = BatchUploadUseCase::new(artifact_processor.clone(), transaction_manager.clone(), 30);

    let commands = vec![
        BatchUploadArtifactCommand {
            coordinates: Default::default(),
            file_name: "test1.txt".to_string(),
            content_length: 100,
        },
        BatchUploadArtifactCommand {
            coordinates: Default::default(),
            file_name: "test2.txt".to_string(),
            content_length: 200,
        },
    ];

    let contents = vec![vec![1, 2, 3], vec![4, 5, 6]];

    let result = use_case.execute_batch(commands, contents).await.unwrap();

    assert_eq!(result.total_count, 2);
    assert_eq!(result.success_count, 2);
    assert_eq!(result.failure_count, 0);
    assert_eq!(result.skipped_count, 0);

    let processed = artifact_processor.processed_artifacts.lock().unwrap();
    assert_eq!(processed.len(), 2);
    assert_eq!(processed[0].0.file_name, "test1.txt");
    assert_eq!(processed[1].0.file_name, "test2.txt");
}

#[tokio::test]
async fn test_execute_batch_partial_failure() {
    let artifact_processor = Arc::new(MockBatchArtifactProcessor::new(true)); // Always fail
    let transaction_manager = Arc::new(MockTransactionManager::new(false));
    
    let use_case = BatchUploadUseCase::new(artifact_processor.clone(), transaction_manager.clone(), 30);

    let commands = vec![
        BatchUploadArtifactCommand {
            coordinates: Default::default(),
            file_name: "test1.txt".to_string(),
            content_length: 100,
        },
    ];

    let contents = vec![vec![1, 2, 3]];

    let result = use_case.execute_batch(commands, contents).await.unwrap();

    assert_eq!(result.total_count, 1);
    assert_eq!(result.success_count, 0);
    assert_eq!(result.failure_count, 1);
    assert_eq!(result.skipped_count, 0);
    assert_eq!(result.results[0].status, BatchUploadStatus::Failed);
}

#[tokio::test]
async fn test_execute_batch_with_transaction_success() {
    let artifact_processor = Arc::new(MockBatchArtifactProcessor::new(false));
    let transaction_manager = Arc::new(MockTransactionManager::new(true));
    
    let use_case = BatchUploadUseCase::new(artifact_processor.clone(), transaction_manager.clone(), 30);

    let commands = vec![
        BatchUploadArtifactCommand {
            coordinates: Default::default(),
            file_name: "test1.txt".to_string(),
            content_length: 100,
        },
    ];

    let contents = vec![vec![1, 2, 3]];

    let result = use_case.execute_batch(commands, contents).await.unwrap();

    assert_eq!(result.success_count, 1);
    assert_eq!(*transaction_manager.transactions_started.lock().unwrap(), 1);
    assert_eq!(*transaction_manager.transactions_committed.lock().unwrap(), 1);
    assert_eq!(*transaction_manager.transactions_rolled_back.lock().unwrap(), 0);
}

#[tokio::test]
async fn test_execute_batch_with_transaction_failure() {
    let artifact_processor = Arc::new(MockBatchArtifactProcessor::new(true)); // Always fail
    let transaction_manager = Arc::new(MockTransactionManager::new(true));
    
    let use_case = BatchUploadUseCase::new(artifact_processor.clone(), transaction_manager.clone(), 30);

    let commands = vec![
        BatchUploadArtifactCommand {
            coordinates: Default::default(),
            file_name: "test1.txt".to_string(),
            content_length: 100,
        },
    ];

    let contents = vec![vec![1, 2, 3]];

    let result = use_case.execute_batch(commands, contents).await.unwrap();

    assert_eq!(result.failure_count, 1);
    assert_eq!(*transaction_manager.transactions_started.lock().unwrap(), 1);
    assert_eq!(*transaction_manager.transactions_committed.lock().unwrap(), 0);
    assert_eq!(*transaction_manager.transactions_rolled_back.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_execute_batch_timeout() {
    struct SlowProcessor;

    #[async_trait::async_trait]
    impl BatchArtifactProcessor for SlowProcessor {
        async fn process_artifact(
            &self,
            _command: BatchUploadArtifactCommand,
            _content: Vec<u8>,
        ) -> Result<super::dto::BatchUploadArtifactResponse, BatchUploadError> {
            sleep(Duration::from_secs(2)).await; // Sleep for 2 seconds
            Ok(super::dto::BatchUploadArtifactResponse {
                hrn: "test".to_string(),
                url: None,
                status: BatchUploadStatus::Success,
                error_message: None,
            })
        }
    }

    let artifact_processor = Arc::new(SlowProcessor);
    let transaction_manager = Arc::new(MockTransactionManager::new(false));
    
    let use_case = BatchUploadUseCase::new(artifact_processor, transaction_manager, 1); // 1 second timeout

    let commands = vec![
        BatchUploadArtifactCommand {
            coordinates: Default::default(),
            file_name: "test1.txt".to_string(),
            content_length: 100,
        },
    ];

    let contents = vec![vec![1, 2, 3]];

    let result = use_case.execute_batch(commands, contents).await.unwrap();

    assert_eq!(result.failure_count, 1);
    assert_eq!(result.results[0].status, BatchUploadStatus::Failed);
    assert!(result.results[0].error_message.as_ref().unwrap().contains("timeout"));
}

#[tokio::test]
async fn test_execute_batch_invalid_input() {
    let artifact_processor = Arc::new(MockBatchArtifactProcessor::new(false));
    let transaction_manager = Arc::new(MockTransactionManager::new(false));
    
    let use_case = BatchUploadUseCase::new(artifact_processor, transaction_manager, 30);

    let commands = vec![
        BatchUploadArtifactCommand {
            coordinates: Default::default(),
            file_name: "test1.txt".to_string(),
            content_length: 100,
        },
    ];

    let contents = vec![]; // Empty contents

    let result = use_case.execute_batch(commands, contents).await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), BatchUploadError::InvalidRequest(_)));
}

#[tokio::test]
async fn test_should_rollback_on_critical_failure() {
    struct CriticalFailureProcessor;

    #[async_trait::async_trait]
    impl BatchArtifactProcessor for CriticalFailureProcessor {
        async fn process_artifact(
            &self,
            _command: BatchUploadArtifactCommand,
            _content: Vec<u8>,
        ) -> Result<super::dto::BatchUploadArtifactResponse, BatchUploadError> {
            Err(BatchUploadError::RepositoryError("Critical failure".to_string()))
        }
    }

    let artifact_processor = Arc::new(CriticalFailureProcessor);
    let transaction_manager = Arc::new(MockTransactionManager::new(true));
    
    let use_case = BatchUploadUseCase::new(artifact_processor, transaction_manager.clone(), 30);

    let commands = vec![
        BatchUploadArtifactCommand {
            coordinates: Default::default(),
            file_name: "test1.txt".to_string(),
            content_length: 100,
        },
    ];

    let contents = vec![vec![1, 2, 3]];

    let result = use_case.execute_batch(commands, contents).await.unwrap();

    assert_eq!(result.failure_count, 1);
    assert_eq!(*transaction_manager.transactions_rolled_back.lock().unwrap(), 1);
}

#[tokio::test]
async fn test_should_not_rollback_on_non_critical_failure() {
    struct NonCriticalFailureProcessor;

    #[async_trait::async_trait]
    impl BatchArtifactProcessor for NonCriticalFailureProcessor {
        async fn process_artifact(
            &self,
            _command: BatchUploadArtifactCommand,
            _content: Vec<u8>,
        ) -> Result<super::dto::BatchUploadArtifactResponse, BatchUploadError> {
            Err(BatchUploadError::ValidationFailed("Non-critical failure".to_string()))
        }
    }

    let artifact_processor = Arc::new(NonCriticalFailureProcessor);
    let transaction_manager = Arc::new(MockTransactionManager::new(true));
    
    let use_case = BatchUploadUseCase::new(artifact_processor, transaction_manager.clone(), 30);

    let commands = vec![
        BatchUploadArtifactCommand {
            coordinates: Default::default(),
            file_name: "test1.txt".to_string(),
            content_length: 100,
        },
    ];

    let contents = vec![vec![1, 2, 3]];

    let result = use_case.execute_batch(commands, contents).await.unwrap();

    assert_eq!(result.failure_count, 1);
    assert_eq!(*transaction_manager.transactions_committed.lock().unwrap(), 1);
    assert_eq!(*transaction_manager.transactions_rolled_back.lock().unwrap(), 0);
}