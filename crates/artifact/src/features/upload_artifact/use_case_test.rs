#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use bytes::Bytes;
    use time::OffsetDateTime;

    use crate::domain::{
        package_version::PackageCoordinates,
        physical_artifact::PhysicalArtifact,
    };
    use crate::features::upload_artifact::{
        use_case::UploadArtifactUseCase,
        dto::UploadArtifactCommand,
        adapter::test::{MockArtifactRepository, MockArtifactStorage, MockEventPublisher},
    };
    use shared::{
        enums::HashAlgorithm,
        hrn::{PhysicalArtifactId, UserId},
        models::ContentHash,
    };

    #[tokio::test]
    async fn test_upload_new_artifact_should_succeed() {
        // Arrange
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage);
        let publisher = Arc::new(MockEventPublisher::new());
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(),
            storage.clone(),
            publisher.clone(),
        );

        let command = UploadArtifactCommand {
            coordinates: PackageCoordinates {
                namespace: Some("com.example".to_string()),
                name: "test-artifact".to_string(),
                version: "1.0.0".to_string(),
                qualifiers: Default::default(),
            },
            file_name: "test.bin".to_string(),
            content_length: 12,
        };
        let content = Bytes::from_static(b"test content");

        // Act
        let result = use_case.execute(command, content).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.hrn.contains("package-version/com.example/test-artifact/1.0.0"));
        
        // Verify side-effects
        assert_eq!(repo.count_physical_artifacts().await, 1);
        assert_eq!(repo.count_package_versions().await, 1);
        assert_eq!(publisher.events.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_upload_existing_artifact_should_create_new_package_version() {
        // Arrange
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage);
        let publisher = Arc::new(MockEventPublisher::new());

        // Pre-populate the repo with an existing physical artifact
        let existing_content = Bytes::from_static(b"existing content");
        let mut hasher = sha2::Sha256::new();
        hasher.update(&existing_content);
        let hash_str = hex::encode(hasher.finalize());

        let physical_artifact = PhysicalArtifact {
            id: PhysicalArtifactId::new(),
            content_hash: ContentHash {
                algorithm: HashAlgorithm::Sha256,
                value: hash_str.clone(),
            },
            location: "s3://mock-bucket/existing".to_string(),
            size_in_bytes: existing_content.len() as u64,
            created_at: OffsetDateTime::now_utc(),
            created_by: UserId::new_system_user(),
        };
        repo.save_physical_artifact(&physical_artifact).await.unwrap();

        let use_case = UploadArtifactUseCase::new(
            repo.clone(),
            storage.clone(),
            publisher.clone(),
        );

        let command = UploadArtifactCommand {
            coordinates: PackageCoordinates {
                namespace: Some("com.example".to_string()),
                name: "test-artifact".to_string(),
                version: "2.0.0".to_string(), // New version
                qualifiers: Default::default(),
            },
            file_name: "test.bin".to_string(),
            content_length: existing_content.len() as u64,
        };

        // Act
        let result = use_case.execute(command, existing_content).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.hrn.contains("package-version/com.example/test-artifact/2.0.0"));

        // Verify side-effects
        assert_eq!(repo.count_physical_artifacts().await, 1); // No new physical artifact
        assert_eq!(repo.count_package_versions().await, 1); // One new package version
        assert_eq!(publisher.events.lock().unwrap().len(), 1);
    }
}
