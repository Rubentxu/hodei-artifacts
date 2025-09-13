use std::sync::Arc;
use tracing::{info, error, debug, info_span};
use tokio::time::{timeout, Duration};

use super::{
    dto::{BatchUploadArtifactCommand, BatchUploadArtifactResponse, BatchUploadResponse, BatchUploadStatus},
    error::BatchUploadError,
    ports::{BatchArtifactProcessor, BatchTransactionManager},
};

pub struct BatchUploadUseCase {
    artifact_processor: Arc<dyn BatchArtifactProcessor>,
    transaction_manager: Arc<dyn BatchTransactionManager>,
    batch_timeout_seconds: u64,
}

impl BatchUploadUseCase {
    pub fn new(
        artifact_processor: Arc<dyn BatchArtifactProcessor>,
        transaction_manager: Arc<dyn BatchTransactionManager>,
        batch_timeout_seconds: u64,
    ) -> Self {
        Self {
            artifact_processor,
            transaction_manager,
            batch_timeout_seconds,
        }
    }

    pub async fn execute_batch(
        &self,
        commands: Vec<BatchUploadArtifactCommand>,
        contents: Vec<Vec<u8>>,
    ) -> Result<BatchUploadResponse, BatchUploadError> {
        if commands.len() != contents.len() {
            return Err(BatchUploadError::InvalidRequest(
                "Commands and contents must have the same length".to_string(),
            ));
        }

        let span = info_span!(
            "batch_upload_execution",
            batch_size = commands.len()
        );
        let _enter = span.enter();

        info!("Starting batch upload with {} artifacts", commands.len());

        let use_transaction = self.transaction_manager.is_transaction_supported().await;
        
        if use_transaction {
            self.transaction_manager.begin_transaction().await?;
        }

        let total_commands = commands.len();
        let mut results = Vec::new();
        let mut success_count = 0;
        let mut failure_count = 0;
        let mut skipped_count = 0;

        for (i, (command, content)) in commands.into_iter().zip(contents.into_iter()).enumerate() {
            let artifact_span = info_span!(
                "batch_artifact_processing",
                artifact_index = i,
                file_name = %command.file_name,
                coordinates = ?command.coordinates
            );
            let _artifact_enter = artifact_span.enter();

            debug!("Processing artifact {} of {}", i + 1, total_commands);

            match timeout(
                Duration::from_secs(self.batch_timeout_seconds),
                self.artifact_processor.process_artifact(command, content),
            ).await {
                Ok(Ok(response)) => {
                    match response.status {
                        BatchUploadStatus::Success => success_count += 1,
                        BatchUploadStatus::Failed => failure_count += 1,
                        BatchUploadStatus::Skipped => skipped_count += 1,
                    }
                    results.push(response);
                }
                Ok(Err(e)) => {
                    error!("Artifact processing failed: {}", e);
                    failure_count += 1;
                    results.push(BatchUploadArtifactResponse {
                        hrn: "".to_string(),
                        url: None,
                        status: BatchUploadStatus::Failed,
                        error_message: Some(e.to_string()),
                    });

                    // If using transaction and this is a critical failure, rollback
                    if use_transaction && self.should_rollback_on_failure(&e) {
                        info!("Critical failure detected, rolling back transaction");
                        self.transaction_manager.rollback_transaction().await?;
                        return Ok(BatchUploadResponse {
                            results,
                            total_count: total_commands,
                            success_count,
                            failure_count,
                            skipped_count,
                        });
                    }
                }
                Err(_) => {
                    error!("Artifact processing timed out");
                    failure_count += 1;
                    results.push(BatchUploadArtifactResponse {
                        hrn: "".to_string(),
                        url: None,
                        status: BatchUploadStatus::Failed,
                        error_message: Some("Processing timeout".to_string()),
                    });
                }
            }
        }

        if use_transaction {
            if failure_count == 0 || !self.should_rollback_on_partial_failure(success_count, failure_count) {
                self.transaction_manager.commit_transaction().await?;
                info!("Batch transaction committed successfully");
            } else {
                self.transaction_manager.rollback_transaction().await?;
                info!("Batch transaction rolled back due to failures");
            }
        }

        let response = BatchUploadResponse {
            results,
            total_count: total_commands,
            success_count,
            failure_count,
            skipped_count,
        };

        info!(
            "Batch upload completed: {} success, {} failed, {} skipped",
            success_count, failure_count, skipped_count
        );

        Ok(response)
    }

    fn should_rollback_on_failure(&self, error: &BatchUploadError) -> bool {
        // Critical errors that should trigger rollback
        matches!(
            error,
            BatchUploadError::TransactionError(_)
                | BatchUploadError::RepositoryError(_)
                | BatchUploadError::StorageError(_)
        )
    }

    fn should_rollback_on_partial_failure(&self, success_count: usize, failure_count: usize) -> bool {
        // Rollback if more than 50% failed or if critical artifacts failed
        failure_count > success_count || failure_count > 0 && success_count == 0
    }
}