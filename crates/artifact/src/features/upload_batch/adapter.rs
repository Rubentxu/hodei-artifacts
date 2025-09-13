use std::sync::Arc;
use async_trait::async_trait;
use bytes::Bytes;

use crate::features::upload_artifact::{
    use_case::UploadArtifactUseCase,
    dto::{UploadArtifactCommand, UploadArtifactResponse},
};

use super::{
    dto::{BatchUploadArtifactCommand, BatchUploadArtifactResponse, BatchUploadStatus},
    error::BatchUploadError,
    ports::BatchArtifactProcessor,
};

pub struct SingleArtifactProcessor {
    upload_use_case: Arc<UploadArtifactUseCase>,
}

impl SingleArtifactProcessor {
    pub fn new(upload_use_case: Arc<UploadArtifactUseCase>) -> Self {
        Self { upload_use_case }
    }
}

#[async_trait]
impl BatchArtifactProcessor for SingleArtifactProcessor {
    async fn process_artifact(
        &self,
        command: BatchUploadArtifactCommand,
        content: Vec<u8>,
    ) -> Result<BatchUploadArtifactResponse, BatchUploadError> {
        let upload_command = UploadArtifactCommand {
            coordinates: command.coordinates,
            file_name: command.file_name,
            content_length: command.content_length,
        };

        match self.upload_use_case.execute(upload_command, Bytes::from(content)).await {
            Ok(UploadArtifactResponse { hrn, url }) => Ok(BatchUploadArtifactResponse {
                hrn,
                url,
                status: BatchUploadStatus::Success,
                error_message: None,
            }),
            Err(e) => Ok(BatchUploadArtifactResponse {
                hrn: "".to_string(),
                url: None,
                status: BatchUploadStatus::Failed,
                error_message: Some(e.to_string()),
            }),
        }
    }
}

pub struct NoopTransactionManager;

#[async_trait]
impl super::ports::BatchTransactionManager for NoopTransactionManager {
    async fn begin_transaction(&self) -> Result<(), BatchUploadError> {
        Ok(())
    }

    async fn commit_transaction(&self) -> Result<(), BatchUploadError> {
        Ok(())
    }

    async fn rollback_transaction(&self) -> Result<(), BatchUploadError> {
        Ok(())
    }

    async fn is_transaction_supported(&self) -> bool {
        false
    }
}

#[cfg(test)]
pub mod test {
    use super::*;
    use std::sync::Mutex;
    use shared::models::PackageCoordinates;

    pub struct MockBatchArtifactProcessor {
        pub processed_artifacts: Mutex<Vec<(BatchUploadArtifactCommand, Vec<u8>)>>,
        pub should_fail: bool,
    }

    impl MockBatchArtifactProcessor {
        pub fn new(should_fail: bool) -> Self {
            Self {
                processed_artifacts: Mutex::new(Vec::new()),
                should_fail,
            }
        }
    }

    #[async_trait]
    impl BatchArtifactProcessor for MockBatchArtifactProcessor {
        async fn process_artifact(
            &self,
            command: BatchUploadArtifactCommand,
            content: Vec<u8>,
        ) -> Result<BatchUploadArtifactResponse, BatchUploadError> {
            self.processed_artifacts.lock().unwrap().push((command, content.clone()));
            
            if self.should_fail {
                Ok(BatchUploadArtifactResponse {
                    hrn: "".to_string(),
                    url: None,
                    status: BatchUploadStatus::Failed,
                    error_message: Some("Mock failure".to_string()),
                })
            } else {
                Ok(BatchUploadArtifactResponse {
                    hrn: "test-hrn".to_string(),
                    url: Some("http://example.com".to_string()),
                    status: BatchUploadStatus::Success,
                    error_message: None,
                })
            }
        }
    }

    pub struct MockTransactionManager {
        pub transactions_started: Mutex<usize>,
        pub transactions_committed: Mutex<usize>,
        pub transactions_rolled_back: Mutex<usize>,
        pub supports_transaction: bool,
    }

    impl MockTransactionManager {
        pub fn new(supports_transaction: bool) -> Self {
            Self {
                transactions_started: Mutex::new(0),
                transactions_committed: Mutex::new(0),
                transactions_rolled_back: Mutex::new(0),
                supports_transaction,
            }
        }
    }

    #[async_trait]
    impl crate::features::upload_batch::ports::BatchTransactionManager for MockTransactionManager {
        async fn begin_transaction(&self) -> Result<(), BatchUploadError> {
            *self.transactions_started.lock().unwrap() += 1;
            Ok(())
        }

        async fn commit_transaction(&self) -> Result<(), BatchUploadError> {
            *self.transactions_committed.lock().unwrap() += 1;
            Ok(())
        }

        async fn rollback_transaction(&self) -> Result<(), BatchUploadError> {
            *self.transactions_rolled_back.lock().unwrap() += 1;
            Ok(())
        }

        async fn is_transaction_supported(&self) -> bool {
            self.supports_transaction
        }
    }
}