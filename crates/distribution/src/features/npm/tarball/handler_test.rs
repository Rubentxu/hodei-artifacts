// Unit tests for handle_npm_tarball_download

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use std::sync::Arc;
    use artifact::application::ports::{ArtifactStorage, ArtifactRepository};
    use iam::application::ports::Authorization;
    use crate::error::DistributionError;
    use cedar_policy::{Request, Response, Decision, PolicyId};
    use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
    use artifact::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion, Ecosystem, PackageCoordinates, Version};
    use std::collections::HashSet;
    use crate::features::npm::tarball::handler::handle_npm_tarball_download;

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
    struct MockArtifactRepository { return_artifacts: Vec<Artifact> }

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
        async fn find_by_maven_coordinates(&self, _group_id: &str, _artifact_id: &str, _version: &str, _file_name: &str) -> Result<Option<Artifact>, artifact::error::ArtifactError> {
            unimplemented!()
        }
        async fn find_by_npm_package_name(&self, _package_name: &str) -> Result<Vec<Artifact>, artifact::error::ArtifactError> {
            Ok(self.return_artifacts.clone())
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

    fn create_mock_artifact(package_name: &str, version: &str, file_name: &str, checksum: &str, artifact_id: ArtifactId) -> Artifact {
        Artifact {
            id: artifact_id,
            repository_id: RepositoryId::new(),
            version: ArtifactVersion::new(version),
            file_name: file_name.to_string(),
            size_bytes: 123,
            checksum: ArtifactChecksum::new(checksum),
            created_at: IsoTimestamp::now(),
            created_by: UserId::new(),
            coordinates: Some(PackageCoordinates::build(
                Ecosystem::Npm,
                None,
                package_name.to_string(),
                version.to_string(),
                None,
                std::collections::BTreeMap::new(),
            ).unwrap()),
        }
    }

    #[tokio::test]
    async fn test_successful_npm_tarball_download() {
        let test_artifact_id = ArtifactId(uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap());
        let package_name = "my-npm-package".to_string();
        let file_name = "my-npm-package-1.0.0.tgz".to_string();
        let artifacts = vec![
            create_mock_artifact(&package_name, "1.0.0", &file_name, "sha1-v1", test_artifact_id),
        ];
        let artifact_storage = Arc::new(MockArtifactStorage { expected_artifact_id: test_artifact_id });
        let artifact_repository = Arc::new(MockArtifactRepository { return_artifacts: artifacts });
        let authorization = Arc::new(MockAuthorization { allow: true });

        let result = handle_npm_tarball_download(
            artifact_storage,
            artifact_repository,
            authorization,
            package_name.clone(),
            file_name.clone(),
        ).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_unauthorized_npm_tarball_download() {
        let test_artifact_id = ArtifactId(uuid::Uuid::parse_str("00000000-0000-0000-0000-000000000000").unwrap());
        let package_name = "my-npm-package".to_string();
        let file_name = "my-npm-package-1.0.0.tgz".to_string();
        let artifacts = vec![
            create_mock_artifact(&package_name, "1.0.0", &file_name, "sha1-v1", test_artifact_id),
        ];
        let artifact_storage = Arc::new(MockArtifactStorage { expected_artifact_id: test_artifact_id });
        let artifact_repository = Arc::new(MockArtifactRepository { return_artifacts: artifacts });
        let authorization = Arc::new(MockAuthorization { allow: false });

        let result = handle_npm_tarball_download(
            artifact_storage,
            artifact_repository,
            authorization,
            package_name.clone(),
            file_name.clone(),
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
    async fn test_npm_tarball_not_found() {
        let test_artifact_id = ArtifactId(uuid::Uuid::new_v4()); // This will not match
        let package_name = "my-npm-package".to_string();
        let file_name = "non-existent-1.0.0.tgz".to_string();
        let artifacts = vec![
            create_mock_artifact(&package_name, "1.0.0", "my-npm-package-1.0.0.tgz", "sha1-v1", test_artifact_id),
        ];
        let artifact_storage = Arc::new(MockArtifactStorage { expected_artifact_id: test_artifact_id });
        let artifact_repository = Arc::new(MockArtifactRepository { return_artifacts: artifacts });
        let authorization = Arc::new(MockAuthorization { allow: true });

        let result = handle_npm_tarball_download(
            artifact_storage,
            artifact_repository,
            authorization,
            package_name.clone(),
            file_name.clone(),
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DistributionError::NotFound => assert!(true),
            _ => panic!("Unexpected error type"),
        }
    }
}
