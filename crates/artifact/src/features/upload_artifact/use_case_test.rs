use bytes::Bytes;
use crate::domain::package_version::PackageCoordinates;
use crate::features::upload_artifact::{UploadArtifactCommand, use_case::UploadArtifactUseCase};
use std::sync::Arc;
use crate::features::upload_artifact::mocks::{MockArtifactRepository, MockArtifactStorage, MockEventPublisher, MockArtifactValidator, MockVersionValidator};

#[tokio::test]
async fn test_upload_with_various_valid_version_formats() {
    // Arrange
    use shared::testing::tracing_utils::setup_test_tracing;
    let _guard = setup_test_tracing();
    let repo = Arc::new(MockArtifactRepository::new());
    let storage = Arc::new(MockArtifactStorage::new());
    let publisher = Arc::new(MockEventPublisher::new());
    let validator = Arc::new(MockArtifactValidator::new());
    let version_validator = Arc::new(MockVersionValidator::new());
    
    // Create mock content type detection service
    use crate::features::content_type_detection::{ContentTypeDetectionUseCase, mocks::MockContentTypeDetector};
    let content_type_detector = Arc::new(MockContentTypeDetector::new());
    let content_type_service = Arc::new(ContentTypeDetectionUseCase::new(content_type_detector));

    let use_case = UploadArtifactUseCase::new(
        repo.clone(),
        storage.clone(),
        publisher.clone(),
        validator.clone(),
        version_validator,
        content_type_service,
    );

    // Test various valid version formats
    let test_cases = vec![
        ("1.0.0", "basic semantic version"),
        ("2.1.3", "multi-digit version"),
        ("0.1.0", "pre-1.0 version"),
        ("1.0.0-alpha", "pre-release version"),
        ("1.0.0-beta.1", "pre-release with number"),
        ("1.0.0-rc.2", "release candidate"),
        ("1.0.0+build.1", "build metadata"),
        ("1.0.0-alpha+build.1", "pre-release with build metadata"),
        ("1.0.0-SNAPSHOT", "snapshot version"),
        ("2.3.4-snapshot", "lowercase snapshot"),
    ];

    for (version, description) in &test_cases {
        let command = UploadArtifactCommand {
            coordinates: PackageCoordinates {
                namespace: Some("example".to_string()),
                name: format!("test-artifact-{}", version.replace('.', "-")),
                version: version.to_string(),
                qualifiers: Default::default(),
            },
            file_name: "test.bin".to_string(),
            content_length: 12,
        };
        let content = Bytes::from_static(b"test content");

        // Act
        let result = use_case.execute(command, content).await;

        // Assert
        assert!(result.is_ok(), "Failed for {}: {:?}", description, result.err());
        let response = result.unwrap();
        assert!(response.hrn.contains(*version), "HRN should contain version {} for {}", version, description);
    }

    // Verify side-effects
    assert_eq!(repo.count_physical_artifacts().await, test_cases.len());
    assert_eq!(repo.count_package_versions().await, test_cases.len());
    assert_eq!(publisher.events.lock().unwrap().len(), test_cases.len());
}

#[tokio::test]
async fn test_upload_with_edge_case_versions() {
    // Arrange
    use shared::testing::tracing_utils::setup_test_tracing;
    let _guard = setup_test_tracing();
    let repo = Arc::new(MockArtifactRepository::new());
    let storage = Arc::new(MockArtifactStorage::new());
    let publisher = Arc::new(MockEventPublisher::new());
    let validator = Arc::new(MockArtifactValidator::new());
    let version_validator = Arc::new(MockVersionValidator::new());
    
    // Create mock content type detection service
    use crate::features::content_type_detection::{ContentTypeDetectionUseCase, mocks::MockContentTypeDetector};
    let content_type_detector = Arc::new(MockContentTypeDetector::new());
    let content_type_service = Arc::new(ContentTypeDetectionUseCase::new(content_type_detector));

    let use_case = UploadArtifactUseCase::new(
        repo.clone(),
        storage.clone(),
        publisher.clone(),
        validator.clone(),
        version_validator,
        content_type_service,
    );

    // Test edge case versions that should work with default validator
    let edge_cases = vec![
        ("999.999.999", "maximum version numbers"),
        ("0.0.1", "minimum non-zero version"),
        ("1.0.0-0", "pre-release with zero"),
        ("1.0.0-0A", "pre-release with alphanumeric"),
        ("1.0.0+A", "build metadata with letters"),
    ];

    for (version, description) in &edge_cases {
        let command = UploadArtifactCommand {
            coordinates: PackageCoordinates {
                namespace: Some("example".to_string()),
                name: format!("edge-test-{}", version.replace('.', "-")),
                version: version.to_string(),
                qualifiers: Default::default(),
            },
            file_name: "test.bin".to_string(),
            content_length: 12,
        };
        let content = Bytes::from_static(b"test content");

        // Act
        let result = use_case.execute(command, content).await;

        // Assert
        assert!(result.is_ok(), "Failed for {}: {:?}", description, result.err());
        let response = result.unwrap();
        assert!(response.hrn.contains(*version), "HRN should contain version {} for {}", version, description);
    }

    // Verify side-effects
    assert_eq!(repo.count_physical_artifacts().await, edge_cases.len());
    assert_eq!(repo.count_package_versions().await, edge_cases.len());
    assert_eq!(publisher.events.lock().unwrap().len(), edge_cases.len());
}