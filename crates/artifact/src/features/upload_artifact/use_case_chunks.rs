use std::sync::Arc;
use tracing::{info, debug, error, info_span};
use tokio::fs;
use std::path::PathBuf;

use super::{
    dto::{UploadArtifactChunkCommand, UploadArtifactCommand, UploadArtifactResponse},
    error::UploadArtifactError,
    ports::{ChunkedUploadStorage, EventPublisher},
    use_case::UploadArtifactUseCase,
};
use crate::domain::events::ArtifactEvent;

#[derive(Debug)]
pub enum ChunkOutcome {
    Accepted { next_expected_chunk: usize },
    Completed { final_path: PathBuf, total_chunks: usize },
}

pub struct UploadArtifactChunkUseCase {
    chunked_storage: Arc<dyn ChunkedUploadStorage + Send + Sync>,
    artifact_use_case: Arc<UploadArtifactUseCase>,
    event_publisher: Arc<dyn EventPublisher + Send + Sync>,
}

impl UploadArtifactChunkUseCase {
    pub fn new(
        chunked_storage: Arc<dyn ChunkedUploadStorage + Send + Sync>,
        artifact_use_case: Arc<UploadArtifactUseCase>,
        event_publisher: Arc<dyn EventPublisher + Send + Sync>,
    ) -> Self {
        Self { chunked_storage, artifact_use_case, event_publisher }
    }

    pub async fn execute(
        &self,
        command: UploadArtifactChunkCommand,
        chunk_data: bytes::Bytes,
    ) -> Result<UploadArtifactResponse, UploadArtifactError> {
        let span = info_span!(
            "upload_artifact_chunk_execution",
            upload_id = %command.upload_id,
            chunk_number = command.chunk_number,
            total_chunks = command.total_chunks,
            file_name = %command.file_name,
        );
        let _enter = span.enter();

        debug!("Executing UploadArtifactChunkUseCase for chunk {} of {}", command.chunk_number, command.total_chunks);

        self.chunked_storage.save_chunk(&command.upload_id, command.chunk_number, chunk_data).await?;
        info!("Chunk {} saved for upload_id {}", command.chunk_number, command.upload_id);

        let received_chunks = self.chunked_storage.get_received_chunks_count(&command.upload_id).await?;
        debug!("Received {} of {} chunks for upload_id {}", received_chunks, command.total_chunks, command.upload_id);

        if received_chunks == command.total_chunks {
            info!("All chunks received for upload_id {}. Assembling file.", command.upload_id);

            // Assemble the file and get the checksum on-the-fly
            let (temp_file_path, checksum) = self.chunked_storage.assemble_chunks(&command.upload_id, command.total_chunks, &command.file_name).await?;
            info!("File assembled to {:?} for upload_id {}. Checksum: {}", temp_file_path, command.upload_id, checksum);

            let coordinates = command.coordinates.ok_or_else(|| UploadArtifactError::BadRequest("Coordinates are required for final artifact assembly.".to_string()))?;

            let cmd = UploadArtifactCommand {
                coordinates: coordinates.clone(),
                file_name: command.file_name.clone(),
                content_length: fs::metadata(&temp_file_path).await.map_err(|e| UploadArtifactError::StorageError(format!("Failed to get assembled file metadata: {}", e)))?.len(),
            };

            // Delegate to the main use case with the pre-calculated checksum
            let result = self.artifact_use_case.execute_from_temp_file(cmd, &temp_file_path, Some(checksum)).await;

            // Cleanup temporary files
            self.chunked_storage.cleanup(&command.upload_id).await?;
            if let Err(e) = fs::remove_file(&temp_file_path).await {
                error!("Failed to remove assembled temporary file {:?}: {}", temp_file_path, e);
            }

            result
        } else {
            // Throttle progress events to every 10%
            let progress_percentage = (received_chunks * 100 / command.total_chunks) as u8;
            if progress_percentage % 10 == 0 {
                info!("Publishing progress update for upload_id {}: {}% complete", command.upload_id, progress_percentage);
                let event = ArtifactEvent::UploadProgressUpdated {
                    upload_id: command.upload_id.clone(),
                    progress: progress_percentage as u64,
                    bytes_uploaded: received_chunks as u64,
                    total_bytes: command.total_chunks as u64,
                };
                self.event_publisher.publish(&event).await?;
            }

            // For intermediate chunks, we return a 202 Accepted response, with the upload_id as the identifier.
            Ok(UploadArtifactResponse { hrn: command.upload_id, url: None })
        }
    }
}
