use async_trait::async_trait;
use std::sync::Arc;

use super::{
    dto::{BatchUploadArtifactCommand, BatchUploadArtifactResponse},
    error::BatchUploadError,
};

#[async_trait]
pub trait BatchArtifactProcessor: Send + Sync {
    async fn process_artifact(
        &self,
        command: BatchUploadArtifactCommand,
        content: Vec<u8>,
    ) -> Result<BatchUploadArtifactResponse, BatchUploadError>;
}

#[async_trait]
pub trait BatchTransactionManager: Send + Sync {
    async fn begin_transaction(&self) -> Result<(), BatchUploadError>;
    async fn commit_transaction(&self) -> Result<(), BatchUploadError>;
    async fn rollback_transaction(&self) -> Result<(), BatchUploadError>;
    async fn is_transaction_supported(&self) -> bool;
}