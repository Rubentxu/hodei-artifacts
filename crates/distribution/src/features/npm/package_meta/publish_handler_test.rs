// Unit tests for handle_npm_publish

#[cfg(test)]
pub mod tests {
    use async_trait::async_trait;
    use std::sync::Arc;
    use artifact::application::ports::{ArtifactStorage, ArtifactRepository, ArtifactEventPublisher, NewArtifactParams};
    use iam::application::ports::Authorization;
    use crate::error::DistributionError;
    use cedar_policy::{Request, Response, Decision, PolicyId};
    use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
    use artifact::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion, Ecosystem, PackageCoordinates, Version};
    use std::collections::HashSet;
    use base64;
    use serde_json::json;
    use crate::features::npm::package_meta::publish_handler::{handle_npm_publish, NpmPublishRequest, NpmAttachment, NpmVersionData, NpmDistData, NpmPublishResponse};

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

    pub fn create_npm_publish_request(package_name: &str, version: &str, tarball_data: &[u8]) -> NpmPublishRequest {
        let encoded_tarball = base64::encode(tarball_data);
        let mut attachments = std::collections::HashMap::new();
        attachments.insert(
            format!("{}-{}.tgz", package_name, version),
            NpmAttachment {
                content_type: "application/octet-stream".to_string(),
                data: encoded_tarball,
                length: tarball_data.len() as u64,
            },
        );

        let mut versions = std::collections::HashMap::new();
        versions.insert(
            version.to_string(),
            NpmVersionData {
                name: package_name.to_string(),
                version: version.to_string(),
                dist: NpmDistData {
                    shasum: "dummy_shasum".to_string(),
                    tarball: "dummy_tarball_url".to_string(),
                },
            },
        );

        NpmPublishRequest {
            id: format!("{}", uuid::Uuid::new_v4()),
            name: package_name.to_string(),
            description: Some("A test package".to_string()),
            dist_tags: std::collections::HashMap::new(),
            versions,
            attachments,
        }
    }

    #[tokio::test]
    async fn test_successful_npm_publish() {
        let artifact_storage = Arc::new(MockArtifactStorage { put_should_fail: false });
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

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.success);
        assert_eq!(response.id, request.id);
    }

    #[tokio::test]
    async fn test_unauthorized_npm_publish() {
        let artifact_storage = Arc::new(MockArtifactStorage { put_should_fail: false });
        let artifact_repository = Arc::new(MockArtifactRepository { save_should_fail: false });
        let artifact_event_publisher = Arc::new(MockArtifactEventPublisher);
        let authorization = Arc::new(MockAuthorization { allow: false });

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
            DistributionError::Iam(iam_err) => {
                assert!(matches!(iam_err, iam::error::IamError::Unauthorized(_)))
            },
            _ => panic!("Unexpected error type"),
        }
    }

    #[tokio::test]
    async fn test_npm_publish_repository_error() {
        let artifact_storage = Arc::new(MockArtifactStorage { put_should_fail: false });
        let artifact_repository = Arc::new(MockArtifactRepository { save_should_fail: true });
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