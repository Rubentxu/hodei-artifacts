use artifact::features::upload_artifact::adapter::LocalFsChunkedUploadStorage;
use artifact::features::upload_artifact::ports::ChunkedUploadStorage as UploadArtifactChunkedStorage;
use artifact::features::upload_progress::dto::ReceivedChunkInfo;
use artifact::features::upload_progress::{
    ports::ChunkedUploadStorage as UploadProgressChunkedStorage, ProgressError,
};
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;
use tempfile::TempDir;

/// Wrapper para adaptar LocalFsChunkedUploadStorage al trait ChunkedUploadStorage de upload_progress
pub struct LocalFsChunkedUploadStorageWrapper {
    inner: Arc<LocalFsChunkedUploadStorage>,
}

impl LocalFsChunkedUploadStorageWrapper {
    pub fn new(temp_dir: &TempDir) -> Self {
        Self {
            inner: Arc::new(LocalFsChunkedUploadStorage::new(
                temp_dir.path().to_path_buf(),
            )),
        }
    }
}

#[async_trait]
impl UploadProgressChunkedStorage for LocalFsChunkedUploadStorageWrapper {
    async fn save_chunk(
        &self,
        upload_id: &str,
        chunk_number: usize,
        data: Bytes,
    ) -> Result<(), ProgressError> {
        self.inner
            .save_chunk(upload_id, chunk_number, data)
            .await
            .map_err(|e| ProgressError::StorageError(e.to_string()))
    }

    async fn get_received_chunks_count(&self, upload_id: &str) -> Result<usize, ProgressError> {
        self.inner
            .get_received_chunks_count(upload_id)
            .await
            .map_err(|e| ProgressError::StorageError(e.to_string()))
    }

    async fn get_received_chunk_numbers(
        &self,
        upload_id: &str,
    ) -> Result<Vec<usize>, ProgressError> {
        self.inner
            .get_received_chunk_numbers(upload_id)
            .await
            .map_err(|e| ProgressError::StorageError(e.to_string()))
    }

    async fn get_received_chunks_info(
        &self,
        upload_id: &str,
    ) -> Result<Vec<ReceivedChunkInfo>, ProgressError> {
        let chunk_numbers = self.get_received_chunk_numbers(upload_id).await?;
        let info: Vec<ReceivedChunkInfo> = chunk_numbers
            .into_iter()
            .map(|number| ReceivedChunkInfo {
                chunk_number: number,
                size: 0, // No tenemos acceso al tamaño real sin leer el archivo
            })
            .collect();
        Ok(info)
    }

    async fn assemble_chunks(
        &self,
        upload_id: &str,
        total_chunks: usize,
        file_name: &str,
    ) -> Result<(std::path::PathBuf, String), ProgressError> {
        self.inner
            .assemble_chunks(upload_id, total_chunks, file_name)
            .await
            .map_err(|e| ProgressError::StorageError(e.to_string()))
    }

    async fn cleanup(&self, upload_id: &str) -> Result<(), ProgressError> {
        self.inner
            .cleanup(upload_id)
            .await
            .map_err(|e| ProgressError::StorageError(e.to_string()))
    }
}
