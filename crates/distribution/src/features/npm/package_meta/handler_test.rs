// Unit tests for handle_npm_package_meta

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use std::sync::Arc;
    use artifact::application::ports::ArtifactRepository;
    use iam::application::ports::Authorization;
    use crate::error::DistributionError;
    use cedar_policy::{Request, Response, Decision, PolicyId};
    use shared::{ArtifactId, RepositoryId, UserId, IsoTimestamp};
    use artifact::domain::model::{Artifact, ArtifactChecksum, ArtifactVersion, Ecosystem, PackageCoordinates, Version};
    use std::collections::HashSet;
    use crate::features::npm::package_meta::handler::{handle_npm_package_meta, NpmPackageMetadata, NpmVersionMetadata, NpmDistMetadata};

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

    fn create_mock_artifact(package_name: &str, version: &str, file_name: &str, checksum: &str) -> Artifact {
        Artifact {
            id: ArtifactId::new(),
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
    async fn test_successful_npm_package_meta() {
        let package_name = "my-npm-package".to_string();
        let artifacts = vec![
            create_mock_artifact(&package_name, "1.0.0", "my-npm-package-1.0.0.tgz", "sha1-v1"),
            create_mock_artifact(&package_name, "1.0.1", "my-npm-package-1.0.1.tgz", "sha1-v2"),
        ];
        let artifact_repository = Arc::new(MockArtifactRepository { return_artifacts: artifacts });
        let authorization = Arc::new(MockAuthorization { allow: true });

        let result = handle_npm_package_meta(
            artifact_repository,
            authorization,
            package_name.clone(),
        ).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.name, package_name);
        assert_eq!(response.versions.len(), 2);
        assert!(response.versions.contains_key("1.0.0"));
        assert!(response.versions.contains_key("1.0.1"));
        assert_eq!(response.versions["1.0.0"].dist.shasum, "sha1-v1");
        assert_eq!(response.versions["1.0.1"].dist.shasum, "sha1-v2");
    }

    #[tokio::test]
    async fn test_unauthorized_npm_package_meta() {
        let package_name = "my-npm-package".to_string();
        let artifacts = vec![
            create_mock_artifact(&package_name, "1.0.0", "my-npm-package-1.0.0.tgz", "sha1-v1"),
        ];
        let artifact_repository = Arc::new(MockArtifactRepository { return_artifacts: artifacts });
        let authorization = Arc::new(MockAuthorization { allow: false });

        let result = handle_npm_package_meta(
            artifact_repository,
            authorization,
            package_name.clone(),
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
    async fn test_npm_package_not_found() {
        let package_name = "non-existent-package".to_string();
        let artifacts = vec![];
        let artifact_repository = Arc::new(MockArtifactRepository { return_artifacts: artifacts });
        let authorization = Arc::new(MockAuthorization { allow: true });

        let result = handle_npm_package_meta(
            artifact_repository,
            authorization,
            package_name.clone(),
        ).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            DistributionError::NotFound => assert!(true),
            _ => panic!("Unexpected error type"),
        }
    }
}
