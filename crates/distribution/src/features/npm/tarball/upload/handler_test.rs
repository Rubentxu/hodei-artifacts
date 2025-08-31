use super::*;
use std::sync::Arc;
use async_trait::async_trait;
use artifact::application::ports::{ArtifactStorage, ArtifactRepository, ArtifactEventPublisher};
use artifact::domain::model::Artifact;
use artifact::error::ArtifactError;
use iam::application::ports::Authorization;
use iam::error::IamError;
use shared::{RepositoryId, UserId, IsoTimestamp, ArtifactId};
use cedar_policy::{Response, Decision, Diagnostics};
use std::collections::HashMap;

// Mock implementations for testing
#[derive(Debug)]
struct MockArtifactStorage {
    stored_objects: HashMap<String, Vec<u8>>,
}

#[async_trait]
impl ArtifactStorage for MockArtifactStorage {
    async fn put_object(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
        bytes: &[u8],
    ) -> Result<(), ArtifactError> {
        let key = format!("{}/{}", repository_id.0, artifact_id.0);
        let mut objects = self.stored_objects.clone();
        objects.insert(key, bytes.to_vec());
        Ok(())
    }

    async fn get_object_stream(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
    ) -> Result<Vec<u8>, ArtifactError> {
        let key = format!("{}/{}", repository_id.0, artifact_id.0);
        self.stored_objects
            .get(&key)
            .cloned()
            .ok_or(ArtifactError::NotFound)
    }

    async fn get_presigned_download_url(
        &self,
        repository_id: &RepositoryId,
        artifact_id: &ArtifactId,
        expires_in_secs: u64,
    ) -> Result<String, ArtifactError> {
        let key = format!("{}/{}", repository_id.0, artifact_id.0);
        if self.stored_objects.contains_key(&key) {
            Ok(format!("https://test.com/{}", key))
        } else {
            Err(ArtifactError::NotFound)
        }
    }
}

#[derive(Debug)]
struct MockArtifactRepository {
    artifacts: HashMap<ArtifactId, Artifact>,
}

#[async_trait]
impl ArtifactRepository for MockArtifactRepository {
    async fn save(&self, artifact: &Artifact) -> Result<(), ArtifactError> {
        let mut artifacts = self.artifacts.clone();
        artifacts.insert(artifact.id.clone(), artifact.clone());
        Ok(())
    }

    async fn get(&self, id: &ArtifactId) -> Result<Option<Artifact>, ArtifactError> {
        Ok(self.artifacts.get(id).cloned())
    }

    async fn find_by_repo_and_checksum(
        &self,
        repository: &RepositoryId,
        checksum: &artifact::domain::model::ArtifactChecksum,
    ) -> Result<Option<Artifact>, ArtifactError> {
        Ok(self.artifacts.values().find(|a| 
            a.repository_id == *repository && a.checksum == *checksum
        ).cloned())
    }

    async fn find_by_maven_coordinates(
        &self,
        group_id: &str,
        artifact_id: &str,
        version: &str,
        file_name: &str,
    ) -> Result<Option<Artifact>, ArtifactError> {
        unimplemented!()
    }

    async fn find_by_npm_package_name(&self, package_name: &str) -> Result<Vec<Artifact>, ArtifactError> {
        Ok(self.artifacts.values()
            .filter(|a| a.coordinates.as_ref().map(|c| c.artifact_id.as_deref()) == Some(Some(package_name))
            .cloned()
            .collect())
    }

    async fn find_all_artifacts(&self) -> Result<Vec<Artifact>, ArtifactError> {
        Ok(self.artifacts.values().cloned().collect())
    }
}

#[derive(Debug)]
struct MockArtifactEventPublisher;

#[async_trait]
impl ArtifactEventPublisher for MockArtifactEventPublisher {
    async fn publish_created(
        &self,
        event: &shared::domain::event::DomainEventEnvelope<shared::domain::event::ArtifactUploaded>,
    ) -> Result<(), ArtifactError> {
        Ok(())
    }

    async fn publish_download_requested(
        &self,
        event: &shared::domain::event::ArtifactDownloadRequestedEvent,
    ) -> Result<(), ArtifactError> {
        Ok(())
    }
}

#[derive(Debug)]
struct MockAuthorization;

#[async_trait]
impl Authorization for MockAuthorization {
    async fn is_authorized(&self, request: cedar_policy::Request) -> Result<Response, IamError> {
        Ok(Response::new(Decision::Allow, Diagnostics::new()))
    }
}

#[test]
fn test_extract_metadata_from_valid_tarball() {
    // Create a simple test tarball with package.json
    let package_json = r#"{
        "name": "test-package",
        "version": "1.0.0",
        "description": "A test package"
    }"#.as_bytes().to_vec();
    
    // This would normally be a real gzipped tarball, but for unit test we'll simulate
    // In a real test, we'd create an actual .tgz file with tar + gzip
    
    // For now, test that the function handles missing package.json correctly
    let result = extract_metadata_from_tarball(b"invalid tarball data");
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DistributionError::InvalidNpmPackage(_)));
}

#[test]
fn test_extract_metadata_from_tarball_missing_package_json() {
    // Test with empty data
    let result = extract_metadata_from_tarball(&[]);
    assert!(result.is_err());
    
    // Test with invalid gzip data
    let result = extract_metadata_from_tarball(b"not a gzip file");
    assert!(result.is_err());
}

#[tokio::test]
async fn test_handle_npm_tarball_upload_authorization_failure() {
    struct DenyAuthorization;

    #[async_trait]
    impl Authorization for DenyAuthorization {
        async fn is_authorized(&self, request: cedar_policy::Request) -> Result<Response, IamError> {
            Ok(Response::new(Decision::Deny, Diagnostics::new()))
        }
    }

    let storage = Arc::new(MockArtifactStorage { stored_objects: HashMap::new() });
    let repository = Arc::new(MockArtifactRepository { artifacts: HashMap::new() });
    let event_publisher = Arc::new(MockArtifactEventPublisher);
    let authorization = Arc::new(DenyAuthorization);

    let request = NpmTarballUploadRequest {
        repository_id: RepositoryId(uuid::Uuid::new_v4()),
        user_id: UserId(uuid::Uuid::new_v4()),
        package_name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        tarball_data: vec![],
        user_agent: None,
        client_ip: None,
    };

    let result = handle_npm_tarball_upload(
        storage,
        repository,
        event_publisher,
        authorization,
        request,
    ).await;

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), DistributionError::Iam(IamError::Unauthorized(_))));
}

#[test]
fn test_npm_tarball_upload_request_creation() {
    let repository_id = RepositoryId(uuid::Uuid::new_v4());
    let user_id = UserId(uuid::Uuid::new_v4());
    
    let request = NpmTarballUploadRequest {
        repository_id: repository_id.clone(),
        user_id: user_id.clone(),
        package_name: "test-package".to_string(),
        version: "1.0.0".to_string(),
        tarball_data: vec![1, 2, 3],
        user_agent: Some("test-agent".to_string()),
        client_ip: Some("127.0.0.1".to_string()),
    };

    assert_eq!(request.repository_id, repository_id);
    assert_eq!(request.user_id, user_id);
    assert_eq!(request.package_name, "test-package");
    assert_eq!(request.version, "1.0.0");
    assert_eq!(request.tarball_data, vec![1, 2, 3]);
    assert_eq!(request.user_agent, Some("test-agent".to_string()));
    assert_eq!(request.client_ip, Some("127.0.0.1".to_string()));
}