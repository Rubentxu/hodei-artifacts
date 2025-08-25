#[cfg(test)]
mod tests {
    use crate::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion};
    use crate::features::download_artifact::query::{GetArtifactQuery, DownloadMethod};
    use crate::features::download_artifact::logic::use_case::{
        execute_download_use_case, DownloadUseCaseResult, build_download_response,
    };
    use crate::application::ports::{ArtifactRepository, ArtifactStorage};
    use crate::error::ArtifactError;
    use shared::{ArtifactId, IsoTimestamp, RepositoryId, UserId};
    use uuid::Uuid;
    use async_trait::async_trait;

    // Mock implementations for testing
    struct MockArtifactRepository {
        artifact: Option<Artifact>,
    }

    #[async_trait]
    impl ArtifactRepository for MockArtifactRepository {
        async fn get(&self, _id: &ArtifactId) -> Result<Option<Artifact>, ArtifactError> {
            Ok(self.artifact.clone())
        }

        async fn save(&self, _artifact: &Artifact) -> Result<(), ArtifactError> {
            unimplemented!()
        }

        async fn find_by_repo_and_checksum(
            &self,
            _repo_id: &RepositoryId,
            _checksum: &ArtifactChecksum,
        ) -> Result<Option<Artifact>, ArtifactError> {
            unimplemented!()
        }
    }

    struct MockArtifactStorage {
        should_fail: bool,
    }

    #[async_trait]
    impl ArtifactStorage for MockArtifactStorage {
        async fn put_object(
            &self,
            _repo_id: &RepositoryId,
            _artifact_id: &ArtifactId,
            _bytes: &[u8],
        ) -> Result<(), ArtifactError> {
            unimplemented!()
        }

        async fn get_object_stream(
            &self,
            _repo_id: &RepositoryId,
            _artifact_id: &ArtifactId,
        ) -> Result<Vec<u8>, ArtifactError> {
            if self.should_fail {
                Err(ArtifactError::StorageDownload("Mock failure".to_string()))
            } else {
                Ok(b"test content".to_vec())
            }
        }

        async fn get_presigned_download_url(
            &self,
            _repo_id: &RepositoryId,
            _artifact_id: &ArtifactId,
            _expires_secs: u64,
        ) -> Result<String, ArtifactError> {
            if self.should_fail {
                Err(ArtifactError::StorageDownload("Mock failure".to_string()))
            } else {
                Ok("https://example.com/presigned-url".to_string())
            }
        }
    }

    fn create_test_artifact() -> Artifact {
        Artifact {
            id: ArtifactId(Uuid::new_v4()),
            repository_id: RepositoryId(Uuid::new_v4()),
            file_name: "test.jar".to_string(),
            size_bytes: 1024,
            checksum: ArtifactChecksum("sha256:abcd1234".to_string()),
            version: ArtifactVersion("1.0.0".to_string()),
            created_by: UserId(Uuid::new_v4()),
            created_at: IsoTimestamp::now(),
            coordinates: None,
        }
    }

    fn create_test_query(use_presigned: bool) -> GetArtifactQuery {
        GetArtifactQuery {
            artifact_id: ArtifactId(Uuid::new_v4()),
            user_id: UserId(Uuid::new_v4()),
            use_presigned_url: use_presigned,
            presigned_expires_secs: if use_presigned { Some(3600) } else { None },
            user_agent: Some("test-agent".to_string()),
            client_ip: Some("127.0.0.1".to_string()),
        }
    }

    #[tokio::test]
    async fn test_execute_download_use_case_direct_success() {
        let artifact = create_test_artifact();
        let repo = MockArtifactRepository {
            artifact: Some(artifact.clone()),
        };
        let storage = MockArtifactStorage { should_fail: false };
        let query = create_test_query(false);

        let result = execute_download_use_case(query, &repo, &storage).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.artifact.id, artifact.id);
        match result.download_method {
            DownloadMethod::Direct { content } => {
                assert_eq!(content, b"test content".to_vec());
            }
            _ => panic!("Expected direct download method"),
        }
    }

    #[tokio::test]
    async fn test_execute_download_use_case_presigned_success() {
        let artifact = create_test_artifact();
        let repo = MockArtifactRepository {
            artifact: Some(artifact.clone()),
        };
        let storage = MockArtifactStorage { should_fail: false };
        let query = create_test_query(true);

        let result = execute_download_use_case(query, &repo, &storage).await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.artifact.id, artifact.id);
        match result.download_method {
            DownloadMethod::PresignedUrl { url, expires_at: _ } => {
                assert_eq!(url, "https://example.com/presigned-url");
            }
            _ => panic!("Expected presigned URL download method"),
        }
    }

    #[tokio::test]
    async fn test_execute_download_use_case_artifact_not_found() {
        let repo = MockArtifactRepository { artifact: None };
        let storage = MockArtifactStorage { should_fail: false };
        let query = create_test_query(false);

        let result = execute_download_use_case(query, &repo, &storage).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ArtifactError::NotFound => {}
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn test_execute_download_use_case_storage_failure() {
        let artifact = create_test_artifact();
        let repo = MockArtifactRepository {
            artifact: Some(artifact),
        };
        let storage = MockArtifactStorage { should_fail: true };
        let query = create_test_query(false);

        let result = execute_download_use_case(query, &repo, &storage).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            ArtifactError::StorageDownload(_) => {}
            _ => panic!("Expected StorageDownload error"),
        }
    }

    #[test]
    fn test_build_download_response() {
        let artifact = create_test_artifact();
        let download_method = DownloadMethod::Direct {
            content: b"test".to_vec(),
        };

        let result = DownloadUseCaseResult {
            artifact: artifact.clone(),
            download_method,
        };

        let response = build_download_response(result);

        assert_eq!(response.artifact_id, artifact.id);
        assert_eq!(response.file_name, artifact.file_name);
        assert_eq!(response.size_bytes, artifact.size_bytes);
        assert_eq!(response.checksum, artifact.checksum.0);
        match response.download_method {
            DownloadMethod::Direct { content } => {
                assert_eq!(content, b"test".to_vec());
            }
            _ => panic!("Expected direct download method"),
        }
    }
}
