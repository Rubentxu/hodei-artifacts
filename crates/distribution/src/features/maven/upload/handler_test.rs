// Unit tests for handle_maven_upload

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use std::sync::Arc;
    use artifact::application::ports::{ArtifactStorage, ArtifactRepository, ArtifactEventPublisher, NewArtifactParams};
    use iam::application::ports::Authorization;
    use crate::error::DistributionError;
    use cedar_policy::{Request, Response, Decision, PolicyId};
    use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
    use artifact::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion, Ecosystem, PackageCoordinates, Version};
    use std::collections::HashSet;
    use crate::features::maven::upload::handler::{handle_maven_upload, UploadResponse};
    use crate::features::npm::package_meta::publish_handler::handle_npm_publish;
    use crate::features::npm::package_meta::publish_handler_test::tests::create_npm_publish_request;

    // Mock ArtifactStorage
    struct MockArtifactStorage { put_should_fail: bool }

    #[async_trait]
    impl ArtifactStorage for MockArtifactStorage {
        async fn put_object(&self, _repository: &RepositoryId, _artifact_id: &ArtifactId, _bytes: &[u8]) -> Result<(), artifact::error::ArtifactError> {
            if self.put_should_fail {
                Err(artifact::error::ArtifactError::Repository("mock put error".to_string()))
            } else {
                Ok(())
            }
        }
        async fn get_object_stream(&self, _repository: &RepositoryId, _artifact_id: &ArtifactId) -> Result<Vec<u8>, artifact::error::ArtifactError> {
            unimplemented!()
        }
        async fn get_presigned_download_url(&self, _repository: &RepositoryId, _artifact_id: &ArtifactId, _expires_in_secs: u64) -> Result<String, artifact::error::ArtifactError> {
            unimplemented!()
        }
    }

    // Mock ArtifactRepository
    struct MockArtifactRepository { save_should_fail: bool }

    #[async_trait]
    impl ArtifactRepository for MockArtifactRepository {
        async fn save(&self, _artifact: &Artifact) -> Result<(), artifact::error::ArtifactError> {
            if self.save_should_fail {
                Err(artifact::error::ArtifactError::Repository("mock save error".to_string()))
            } else {
                Ok(())
            }
        }
        async fn get(&self, _id: &ArtifactId) -> Result<Option<Artifact>, artifact::error::ArtifactError> {
            unimplemented!()
        }
        async fn find_by_repo_and_checksum(&self, _repository: &RepositoryId, _checksum: &artifact::domain::model::ArtifactChecksum) -> Result<Option<Artifact>, artifact::error::ArtifactError> {
            unimplemented!()
        }
        async fn find_by_maven_coordinates(&self, _group_id: &str, _artifact_id: &str, _version: &str, _file_name: &str) -> Result<Option<Artifact>, artifact::error::ArtifactError> {
            unimplemented!()
        }
        async fn find_by_npm_package_name(&self, _package_name: &str) -> Result<Vec<Artifact>, artifact::error::ArtifactError> {
            unimplemented!()
        }
    }

    // Mock ArtifactEventPublisher
    struct MockArtifactEventPublisher;

    #[async_trait]
    impl ArtifactEventPublisher for MockArtifactEventPublisher {
        async fn publish_created(&self, _artifact: &Artifact) -> Result<(), artifact::error::ArtifactError> {
            Ok(())
        }
        async fn publish_download_requested(&self, _event: &shared::domain::event::ArtifactDownloadRequestedEvent) -> Result<(), artifact::error::ArtifactError> {
            unimplemented!()
        }
    }

    // Mock Authorization
    struct MockAuthorization { allow: bool }

    #[async_trait]
    impl Authorization for MockAuthorization {
        async fn is_authorized(&self, _request: Request) -> Result<Response, iam::error::IamError> {
            Ok(Response::new(if self.allow { Decision::Allow } else { Decision::Deny }, HashSet::new(), vec![]))
        }
    }

    #[tokio::test]
    async fn test_successful_upload() {
        let artifact_storage = Arc::new(MockArtifactStorage { put_should_fail: false });
        let artifact_repository = Arc::new(MockArtifactRepository { save_should_fail: false });
        let artifact_event_publisher = Arc::new(MockArtifactEventPublisher);
        let authorization = Arc::new(MockAuthorization { allow: true });

        let result = handle_maven_upload(
            artifact_storage,
            artifact_repository,
            artifact_event_publisher,
            authorization,
            "com.example".to_string(),
            "my-lib".to_string(),
            "1.0.0".to_string(),
            "my-lib-1.0.0.jar".to_string(),
            vec![1, 2, 3, 4],
        ).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.message, "Artifact uploaded successfully");
        assert!(!response.artifact_id.is_empty());
        assert!(!response.sha256.is_empty());
        assert!(!response.md5.is_empty());
    }

    #[tokio::test]
    async fn test_unauthorized_upload() {
        let artifact_storage = Arc::new(MockArtifactStorage { put_should_fail: false });
        let artifact_repository = Arc::new(MockArtifactRepository { save_should_fail: false });
        let artifact_event_publisher = Arc::new(MockArtifactEventPublisher);
        let authorization = Arc::new(MockAuthorization { allow: false });

        let result = handle_maven_upload(
            artifact_storage,
            artifact_repository,
            artifact_event_publisher,
            authorization,
            "com.example".to_string(),
            "my-lib".to_string(),
            "1.0.0".to_string(),
            "my-lib-1.0.0.jar".to_string(),
            vec![1, 2, 3, 4],
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DistributionError::Iam(iam_err) => {
                assert!(matches!(iam_err, iam::error::IamError::Unauthorized(_)))
            },
            _ => panic!("Unexpected error type"),
        }
    }

    #[tokio::test]
    async fn test_upload_repository_error() {
        let artifact_storage = Arc::new(MockArtifactStorage { put_should_fail: false });
        let artifact_repository = Arc::new(MockArtifactRepository { save_should_fail: true });
        let artifact_event_publisher = Arc::new(MockArtifactEventPublisher);
        let authorization = Arc::new(MockAuthorization { allow: true });

        let result = handle_maven_upload(
            artifact_storage,
            artifact_repository,
            artifact_event_publisher,
            authorization,
            "com.example".to_string(),
            "my-lib".to_string(),
            "1.0.0".to_string(),
            "my-lib-1.0.0.jar".to_string(),
            vec![1, 2, 3, 4],
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DistributionError::Artifact(artifact_err) => {
                assert!(matches!(artifact_err, artifact::error::ArtifactError::Repository(_)))
            },
            _ => panic!("Unexpected error type"),
        }
    }

    #[tokio::test]
    async fn test_npm_publish_storage_error() {
        let artifact_storage = Arc::new(MockArtifactStorage { put_should_fail: true });
        let artifact_repository = Arc::new(MockArtifactRepository { save_should_fail: false });
        let artifact_event_publisher = Arc::new(MockArtifactEventPublisher);
        let authorization = Arc::new(MockAuthorization { allow: true });

        let package_name = "test-package".to_string();
        let version = "1.0.0".to_string();
        let tarball_data = vec![1, 2, 3, 4];
        let request = create_npm_publish_request(&package_name, &version, &tarball_data);
        let bytes = serde_json::to_vec(&request).unwrap();

        let result = handle_npm_publish(
            artifact_storage,
            artifact_repository,
            artifact_event_publisher,
            authorization,
            package_name.clone(),
            bytes,
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DistributionError::Artifact(artifact_err) => {
                assert!(matches!(artifact_err, artifact::error::ArtifactError::Repository(_)))
            },
            _ => panic!("Unexpected error type"),
        }
    }
}