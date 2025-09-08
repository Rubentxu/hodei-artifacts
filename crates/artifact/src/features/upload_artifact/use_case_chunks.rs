use std::path::PathBuf;
use std::sync::Arc;
use bytes::Bytes;
use time::OffsetDateTime;
use crate::domain::events::{ArtifactEvent, UploadProgressUpdated};
use super::ports::{ChunkedUploadStorage, EventPublisher, PortResult};

pub struct UploadArtifactChunkUseCase {
    storage: Arc<dyn ChunkedUploadStorage>,
    publisher: Arc<dyn EventPublisher>,
}

impl UploadArtifactChunkUseCase {
    pub fn new(storage: Arc<dyn ChunkedUploadStorage>, publisher: Arc<dyn EventPublisher>) -> Self {
        Self { storage, publisher }
    }

    pub async fn store_chunk(&self,
        upload_id: &str,
        chunk_number: u64,
        chunks_total: Option<u64>,
        data: Bytes,
    ) -> PortResult<ChunkOutcome> {
        let received_len = data.len() as u64;
        self.storage.save_chunk(upload_id, chunk_number, data).await?;

        // Emit progress event (incremental)
        let progress = ArtifactEvent::UploadProgressUpdated(UploadProgressUpdated {
            upload_id: upload_id.to_string(),
            received_bytes: received_len,
            total_bytes: chunks_total.map(|t| t),
            at: OffsetDateTime::now_utc(),
        });
        // Ignorar errores de publicación de progreso (no críticos para flujo)
        let _ = self.publisher.publish(&progress).await;

        if let Some(total) = chunks_total {
            // Si es el último chunk y todos presentes, intentar ensamblar
            if chunk_number == total {
                let final_path = self.storage.assemble_to_path(upload_id, total).await?;
                return Ok(ChunkOutcome::Completed { final_path, total_chunks: total });
            }
            Ok(ChunkOutcome::Accepted { next_expected_chunk: chunk_number + 1 })
        } else {
            Ok(ChunkOutcome::Accepted { next_expected_chunk: chunk_number + 1 })
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChunkOutcome {
    Accepted { next_expected_chunk: u64 },
    Completed { final_path: PathBuf, total_chunks: u64 },
}

