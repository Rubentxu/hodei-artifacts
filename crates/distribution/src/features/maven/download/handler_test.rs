// Unit tests for handle_maven_download

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use std::sync::Arc;
    use artifact::application::ports::{ArtifactStorage, ArtifactRepository};
    use iam::application::ports::Authorization;
    use crate::error::DistributionError;
    use cedar_policy::{Request, Response, Decision, PolicyId};
    use shared::{ArtifactId, RepositoryId};
    use artifact::domain::model::Artifact;
    use std::collections::HashSet;
    use crate::features::maven::download::handler::handle_maven_download;

    // Mock ArtifactStorage
    struct MockArtifactStorage { expected_artifact_id: ArtifactId }

    #[async_trait]
    impl ArtifactStorage for MockArtifactStorage {
        async fn put_object(&self, _repository: &RepositoryId, _artifact_id: &ArtifactId, _bytes: &[u8]) -> Result<(), artifact::error::ArtifactError> {
            unimplemented!()
        }
        async fn get_object_stream(&self, repository: &RepositoryId, artifact_id: &ArtifactId) -> Result<Vec<u8>, artifact::error::ArtifactError> {
            if artifact_id == &self.expected_artifact_id {
                Ok(vec![1, 2, 3, 4])
            } else {
                Err(artifact::error::ArtifactError::NotFound)
            }
        }
        async fn get_presigned_download_url(&self, _repository: &RepositoryId, _artifact_id: &ArtifactId, _expires_in_secs: u64) -> Result<String, artifact::error::ArtifactError> {
            unimplemented!()
        }
    }

    // Mock ArtifactRepository
    struct MockArtifactRepository { return_artifact_id: ArtifactId }

    #[async_trait]
    impl ArtifactRepository for MockArtifactRepository {
        async fn save(&self, _artifact: &Artifact) -> Result<(), artifact::error::ArtifactError> {
            unimplemented!()
        }
        async fn get(&self, _id: &ArtifactId) -> Result<Option<Artifact>, artifact::error::ArtifactError> {
            unimplemented!()
        }
        async fn find_by_repo_and_checksum(&self, _repository: &RepositoryId, _checksum: &artifact::domain::model::ArtifactChecksum) -> Result<Option<Artifact>, artifact::error::ArtifactError> {
            unimplemented!()
        }
        async fn find_by_maven_coordinates(&self, group_id: &str, artifact_id: &str, version: &str, file_name: &str) -> Result<Option<Artifact>, artifact::error::ArtifactError> {
            if group_id == "com.example" && artifact_id == "my-lib" && version == "1.0.0" && file_name == "my-lib-1.0.0.jar" {
                Ok(Some(Artifact {
                    id: self.return_artifact_id,
                    repository_id: RepositoryId::new(),
                    version: artifact::domain::model::ArtifactVersion::new("1.0.0"),
                    file_name: "my-lib-1.0.0.jar".to_string(),
                    size_bytes: 123,
                    checksum: artifact::domain::model::ArtifactChecksum::new("sha256"),
                    created_at: shared::IsoTimestamp::now(),
                    created_by: shared::UserId::new(),
                    coordinates: None,
                }))
            } else {
                Ok(None)
            }
        }
        async fn find_by_npm_package_name(&self, _package_name: &str) -> Result<Vec<Artifact>, artifact::error::ArtifactError> {
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
    async fn test_successful_download() {
        let test_artifact_id = ArtifactId(uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap());
        let artifact_storage = Arc::new(MockArtifactStorage { expected_artifact_id: test_artifact_id });
        let artifact_repository = Arc::new(MockArtifactRepository { return_artifact_id: test_artifact_id });
        let authorization = Arc::new(MockAuthorization { allow: true });

        let result = handle_maven_download(
            artifact_storage,
            artifact_repository,
            authorization,
            "com.example".to_string(),
            "my-lib".to_string(),
            "1.0.0".to_string(),
            "my-lib-1.0.0.jar".to_string(),
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_unauthorized_download() {
        let test_artifact_id = ArtifactId(uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap());
        let artifact_storage = Arc::new(MockArtifactStorage { expected_artifact_id: test_artifact_id });
        let artifact_repository = Arc::new(MockArtifactRepository { return_artifact_id: test_artifact_id });
        let authorization = Arc::new(MockAuthorization { allow: false });

        let result = handle_maven_download(
            artifact_storage,
            artifact_repository,
            authorization,
            "com.example".to_string(),
            "my-lib".to_string(),
            "1.0.0".to_string(),
            "my-lib-1.0.0.jar".to_string(),
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
    async fn test_artifact_not_found() {
        let test_artifact_id = ArtifactId(uuid::Uuid::new_v4()); // This will not match
        let artifact_storage = Arc::new(MockArtifactStorage { expected_artifact_id: test_artifact_id });
        let artifact_repository = Arc::new(MockArtifactRepository { return_artifact_id: test_artifact_id });
        let authorization = Arc::new(MockAuthorization { allow: true });

        let result = handle_maven_download(
            artifact_storage,
            artifact_repository,
            authorization,
            "com.example".to_string(),
            "non-existent".to_string(),
            "1.0.0".to_string(),
            "non-existent-1.0.0.jar".to_string(),
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DistributionError::NotFound => assert!(true),
            _ => panic!("Unexpected error type"),
        }
    }
}