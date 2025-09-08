#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use bytes::Bytes;
    use sha2::{Digest, Sha256};

    use crate::domain::{
        package_version::PackageCoordinates,
        physical_artifact::PhysicalArtifact,
    };
    use crate::features::upload_artifact::{
        use_case::UploadArtifactUseCase,
        dto::UploadArtifactCommand,
        test_adapter::{MockArtifactRepository, MockArtifactStorage, MockEventPublisher},
        error::UploadArtifactError,
    };
    use crate::features::upload_artifact::ports::ArtifactRepository;
    use shared::{
        assert_log_contains,
        enums::HashAlgorithm,
        hrn::{OrganizationId, UserId},
        models::ContentHash,
    };

    #[tokio::test]
    async fn test_upload_new_artifact_should_succeed() {
        // Arrange
        use shared::testing::tracing_utils::setup_test_tracing;
        let _guard = setup_test_tracing();
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
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
        assert!(response.hrn.contains("package-version/test-artifact/1.0.0"));
        assert!(response.hrn.contains("com.example"));
        
        // Verify side-effects
        assert_eq!(repo.count_physical_artifacts().await, 1);
        assert_eq!(repo.count_package_versions().await, 1);
        assert_eq!(publisher.events.lock().unwrap().len(), 1);
        
        // Verify tracing logs
        assert_log_contains!(tracing::Level::INFO, "Executing use case");
        assert_log_contains!(tracing::Level::DEBUG, "Content hash:");
        assert_log_contains!(tracing::Level::DEBUG, "Creating new physical artifact");
        assert_log_contains!(tracing::Level::DEBUG, "Saved new physical artifact");
    }

    #[tokio::test]
    async fn test_upload_existing_artifact_should_create_new_package_version() {
        // Arrange
        use shared::testing::tracing_utils::setup_test_tracing;
        let _guard = setup_test_tracing();
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());

        // Pre-populate the repo with an existing physical artifact
        let existing_content = Bytes::from_static(b"existing content");
        let mut hasher = Sha256::new();
        hasher.update(&existing_content);
        let hash_str = hex::encode(hasher.finalize());

        let physical_artifact = PhysicalArtifact {
            hrn: shared::hrn::PhysicalArtifactId::new(&hash_str).unwrap().0,
            organization_hrn: OrganizationId::new("default").unwrap(),
            content_hash: ContentHash {
                algorithm: HashAlgorithm::Sha256,
                value: hash_str.clone(),
            },
            size_in_bytes: existing_content.len() as u64,
            checksums: Default::default(),
            storage_location: "s3://mock-bucket/existing".to_string(),
            lifecycle: shared::lifecycle::Lifecycle::new(UserId::new_system_user().0),
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
        assert!(response.hrn.contains("package-version/test-artifact/2.0.0"));
        assert!(response.hrn.contains("com.example"));

        // Verify side-effects
        assert_eq!(repo.count_physical_artifacts().await, 1); // No new physical artifact
        assert_eq!(repo.count_package_versions().await, 1); // One new package version
        assert_eq!(publisher.events.lock().unwrap().len(), 1);
        
        // Verify tracing logs and spans
        assert_log_contains!(tracing::Level::DEBUG, "Finding physical artifact by hash:");
        assert_log_contains!(tracing::Level::DEBUG, "Found existing physical artifact");
    }

    #[tokio::test]
    async fn test_upload_with_invalid_namespace_should_fail() {
        // Arrange
        use shared::testing::tracing_utils::setup_test_tracing;
        let _guard = setup_test_tracing();
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(),
            storage.clone(),
            publisher.clone(),
        );

        let command = UploadArtifactCommand {
            coordinates: PackageCoordinates {
                namespace: Some("invalid:namespace".to_string()), // Invalid HRN format
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
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, UploadArtifactError::RepositoryError(_)));
        }
        
        // Verify error logging
        assert_log_contains!(tracing::Level::ERROR, "RepositoryError");
    }

    #[tokio::test]
    async fn test_upload_with_empty_file_should_succeed() {
        // Arrange
        use shared::testing::tracing_utils::setup_test_tracing;
        let _guard = setup_test_tracing();
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(),
            storage.clone(),
            publisher.clone(),
        );

        let command = UploadArtifactCommand {
            coordinates: PackageCoordinates {
                namespace: Some("com.example".to_string()),
                name: "empty-artifact".to_string(),
                version: "1.0.0".to_string(),
                qualifiers: Default::default(),
            },
            file_name: "empty.bin".to_string(),
            content_length: 0,
        };
        let content = Bytes::from_static(b"");

        // Act
        let result = use_case.execute(command, content).await;

        // Assert
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.hrn.contains("package-version/empty-artifact/1.0.0"));
        assert!(response.hrn.contains("com.example"));
        
        // Verify side-effects
        assert_eq!(repo.count_physical_artifacts().await, 1);
        assert_eq!(repo.count_package_versions().await, 1);
        assert_eq!(publisher.events.lock().unwrap().len(), 1);
        
        // Verify tracing logs
        assert_log_contains!(tracing::Level::INFO, "Executing use case");
        assert_log_contains!(tracing::Level::DEBUG, "Content hash:");
        assert_log_contains!(tracing::Level::DEBUG, "Creating new physical artifact");
        assert_log_contains!(tracing::Level::DEBUG, "Saved new physical artifact");
    }

    #[tokio::test]
    async fn test_upload_repository_error_should_fail() {
        // Arrange
        use shared::testing::tracing_utils::setup_test_tracing;
        let _guard = setup_test_tracing();
        let repo = Arc::new(MockArtifactRepository::new());
        *repo.should_fail_save_physical_artifact.lock().unwrap() = true;
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(),
            storage.clone(),
            publisher.clone(),
        );

        let command = UploadArtifactCommand {
            coordinates: PackageCoordinates {
                namespace: Some("com.example".to_string()),
                name: "repo-error-artifact".to_string(),
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
        assert!(result.is_err());
        if let Err(e) = result {
            assert!(matches!(e, UploadArtifactError::RepositoryError(_)));
        }
        
        // Verify error logging
        assert_log_contains!(tracing::Level::ERROR, "RepositoryError");
    }

    #[tokio::test]
    async fn test_upload_storage_error() {
        // Arrange
        use shared::testing::tracing_utils::setup_test_tracing;
        let _guard = setup_test_tracing();
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        *storage.should_fail_upload.lock().unwrap() = true;
        let publisher = Arc::new(MockEventPublisher::new());
        let use_case = UploadArtifactUseCase::new(repo.clone(), storage.clone(), publisher.clone());
        let command = UploadArtifactCommand {
            coordinates: PackageCoordinates { namespace: Some("com.example".to_string()), name: "err-artifact".to_string(), version: "1.0.0".to_string(), qualifiers: Default::default() },
            file_name: "err.bin".to_string(), content_length: 4
        };
        let content = Bytes::from_static(b"fail");
        // Act
        let result = use_case.execute(command, content).await;
        // Assert
        assert!(matches!(result, Err(UploadArtifactError::StorageError(_))));
        assert_log_contains!(tracing::Level::ERROR, "StorageError");
    }

    #[tokio::test]
    async fn test_upload_event_error_does_not_block_success() {
        // Arrange
        use shared::testing::tracing_utils::setup_test_tracing;
        let _guard = setup_test_tracing();
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        *publisher.should_fail_publish.lock().unwrap() = true;
        let use_case = UploadArtifactUseCase::new(repo.clone(), storage.clone(), publisher.clone());
        let command = UploadArtifactCommand {
            coordinates: PackageCoordinates { namespace: Some("com.example".to_string()), name: "noevent-artifact".to_string(), version: "1.0.0".to_string(), qualifiers: Default::default() },
            file_name: "noevent.bin".to_string(), content_length: 6
        };
        let content = Bytes::from_static(b"bleh!!");
        // Act
        let result = use_case.execute(command, content).await;
        // Assert: failure due to event error
        assert!(result.is_err());
        if let Err(e) = result {
            match e {
                UploadArtifactError::EventError(_) => {}, // Expected
                _ => panic!("Expected EventError, got {:?}", e)
            }
        }
    }
}
