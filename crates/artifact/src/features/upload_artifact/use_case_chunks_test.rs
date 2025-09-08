#[cfg(test)]
mod tests {
    use super::use_case_chunks::{UploadArtifactChunkUseCase, ChunkOutcome};
    use super::adapter::LocalChunkedUploadStorage;
    use super::ports::ChunkedUploadStorage;
    use super::adapter::LocalChunkedUploadStorage as StorageImpl;
    use crate::features::upload_artifact::adapter::LocalChunkedUploadStorage as AdapterStorage;
    use bytes::Bytes;
    use std::sync::Arc;
    use crate::features::upload_artifact::ports::PortResult;
    use crate::domain::events::ArtifactEvent;
    use crate::features::upload_artifact::ports::EventPublisher;
    use std::sync::Mutex;
    use time::OffsetDateTime;

    struct DummyPublisher {
        pub events: Mutex<Vec<ArtifactEvent>>,
    }
    #[async_trait::async_trait]
    impl EventPublisher for DummyPublisher {
        async fn publish(&self, event: &ArtifactEvent) -> PortResult<()> {
            self.events.lock().unwrap().push(event.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_storage_save_assemble_cleanup() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = AdapterStorage::new(tmp.path());
        // Save chunks 1 and 2
        storage.save_chunk("test", 1, Bytes::from("foo")).await.unwrap();
        storage.save_chunk("test", 2, Bytes::from("bar")).await.unwrap();
        // Assemble
        let path = storage.assemble_to_path("test", 2).await.unwrap();
        let data = tokio::fs::read_to_string(&path).await.unwrap();
        assert_eq!(data, "foobar");
        // Cleanup
        storage.cleanup_upload("test").await.unwrap();
        assert!(!tmp.path().join("test").exists());
    }

    #[tokio::test]
    async fn test_chunk_use_case_accept_and_complete() {
        let tmp = tempfile::tempdir().unwrap();
        let storage = Arc::new(AdapterStorage::new(tmp.path()));
        let publisher = Arc::new(DummyPublisher { events: Mutex::new(vec![]) });
        let use_case = UploadArtifactChunkUseCase::new(storage.clone(), publisher.clone());
        // First chunk
        let outcome1 = use_case.store_chunk("u1", 1, Some(2), Bytes::from("foo")).await.unwrap();
        match outcome1 {
            ChunkOutcome::Accepted { next_expected_chunk } => assert_eq!(next_expected_chunk, 2),
            _ => panic!("Expected Accepted"),
        }
        // Second chunk -> complete
        let outcome2 = use_case.store_chunk("u1", 2, Some(2), Bytes::from("bar")).await.unwrap();
        match outcome2 {
            ChunkOutcome::Completed { final_path, total_chunks } => {
                assert_eq!(total_chunks, 2);
                let data = tokio::fs::read_to_string(&final_path).await.unwrap();
                assert_eq!(data, "foobar");
            }
            _ => panic!("Expected Completed"),
        }
        // Publisher got two progress events
        let evs = publisher.events.lock().unwrap();
        assert_eq!(evs.len(), 2);
        // Cleanup
        let exists = tmp.path().join("u1").exists();
        assert!(!exists);
    }
}
