#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use axum::{
        body::Body,
        extract::{Multipart, FromRequest},
        http::{Request, StatusCode},
        response::IntoResponse,
        Extension,
    };
    use axum::body::to_bytes;
    use serde_json::json;
    use tracing_test::traced_test;

    use crate::features::upload_artifact::api::UploadArtifactEndpoint;
    use crate::features::upload_artifact::{
        use_case::UploadArtifactUseCase,
        mocks::{MockArtifactRepository, MockArtifactStorage, MockEventPublisher, MockArtifactValidator, MockVersionValidator},
    };
    use crate::features::content_type_detection::{ContentTypeDetectionUseCase, mocks::MockContentTypeDetector};

    #[tokio::test]
    #[traced_test]
    async fn test_upload_artifact_success() {
        // Arrange
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        let validator = Arc::new(MockArtifactValidator::new());
        
        // Create mock content type detection service
        let content_type_detector = Arc::new(MockContentTypeDetector::new());
        let content_type_service = Arc::new(ContentTypeDetectionUseCase::new(content_type_detector));
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(), 
            storage.clone(), 
            publisher.clone(), 
            validator.clone(),
            Arc::new(MockVersionValidator::new()),
            content_type_service,
        );
        let endpoint = UploadArtifactEndpoint::new(Arc::new(use_case));

        let form_data = "--boundary\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{\"coordinates\":{\"namespace\":\"example\",\"name\":\"test-artifact\",\"version\":\"1.0.0\",\"qualifiers\":{}},\"file_name\":\"test.bin\"}\r\n--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.bin\"\r\nContent-Type: application/octet-stream\r\n\r\ntest content\r\n--boundary--\r\n";

        let request = Request::builder()
            .method("POST")
            .uri("/artifacts")
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(form_data))
            .unwrap();

        // Convert Request to Multipart - need to use the proper extractor configuration
        let mut extractor_config = axum::extract::DefaultBodyLimit::max(1024 * 1024); // 1MB limit
        let multipart = Multipart::from_request(request, &mut extractor_config).await.unwrap();

        // Act
        let response = UploadArtifactEndpoint::upload_artifact(Extension(Arc::new(endpoint)), multipart).await;

        // Assert - convert to Response to access status and body
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::CREATED);
        
        let (_parts, body) = response.into_parts();
        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        let response_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        assert!(response_json["hrn"].as_str().unwrap().contains("package-version/test-artifact/1.0.0"));

        // Verify tracing logs and spans
        assert!(logs_contain("Processing upload command:"));
        assert!(logs_contain("Content length: 12"));
        assert!(logs_contain("Upload completed successfully:"));
        assert!(logs_contain("upload_artifact_execution"));
        assert!(logs_contain("file_name=test.bin"));
        assert!(logs_contain("content_length=12"));

        // Verify events were published
        let events = publisher.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], crate::domain::events::ArtifactEvent::PackageVersionPublished(_)));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_upload_artifact_missing_metadata() {
        // Arrange
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        let validator = Arc::new(MockArtifactValidator::new());
        
        // Create mock content type detection service
        let content_type_detector = Arc::new(MockContentTypeDetector::new());
        let content_type_service = Arc::new(ContentTypeDetectionUseCase::new(content_type_detector));
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(), 
            storage.clone(), 
            publisher.clone(), 
            validator.clone(),
            Arc::new(MockVersionValidator::new()),
            content_type_service,
        );
        let endpoint = UploadArtifactEndpoint::new(Arc::new(use_case));

        let form_data = "--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.bin\"\r\nContent-Type: application/octet-stream\r\n\r\ntest content\r\n--boundary--\r\n";

        let request = Request::builder()
            .method("POST")
            .uri("/artifacts")
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(form_data))
            .unwrap();

        // Convert Request to Multipart - need to use the proper extractor configuration
        let mut extractor_config = axum::extract::DefaultBodyLimit::max(1024 * 1024); // 1MB limit
        let multipart = Multipart::from_request(request, &mut extractor_config).await.unwrap();

        // Act
        let response = UploadArtifactEndpoint::upload_artifact(Extension(Arc::new(endpoint)), multipart).await;

        // Assert - convert to Response first
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let (_parts, body) = response.into_parts();
        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        assert_eq!(body_bytes, "Missing metadata or file part");

        // Verify error logging
        assert!(logs_contain("Missing metadata or file part in upload request"));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_upload_artifact_missing_file() {
        // Arrange
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        let validator = Arc::new(MockArtifactValidator::new());
        
        // Create mock content type detection service
        let content_type_detector = Arc::new(MockContentTypeDetector::new());
        let content_type_service = Arc::new(ContentTypeDetectionUseCase::new(content_type_detector));
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(), 
            storage.clone(), 
            publisher.clone(), 
            validator.clone(),
            Arc::new(MockVersionValidator::new()),
            content_type_service,
        );
        let endpoint = UploadArtifactEndpoint::new(Arc::new(use_case));

        let metadata = json!({
            "coordinates": {
                "namespace": "example",
                "name": "test-artifact",
                "version": "1.0.0",
                "qualifiers": {}
            },
            "file_name": "test.bin"
        });

        let form_data = format!(
            "--boundary\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{}\r\n--boundary--\r\n",
            metadata.to_string()
        );

        let request = Request::builder()
            .method("POST")
            .uri("/artifacts")
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(form_data))
            .unwrap();

        // Convert Request to Multipart - need to use the proper extractor configuration
        let mut extractor_config = axum::extract::DefaultBodyLimit::max(1024 * 1024); // 1MB limit
        let multipart = Multipart::from_request(request, &mut extractor_config).await.unwrap();

        // Act
        let response = UploadArtifactEndpoint::upload_artifact(Extension(Arc::new(endpoint)), multipart).await;

        // Assert - convert to Response first
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let (_parts, body) = response.into_parts();
        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        assert_eq!(body_bytes, "Missing metadata or file part");
    }

    #[tokio::test]
    #[traced_test]
    async fn test_upload_artifact_repository_error() {
        // Arrange
        let repo = Arc::new(MockArtifactRepository::new());
        *repo.should_fail_save_physical_artifact.lock().unwrap() = true;
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        let validator = Arc::new(MockArtifactValidator::new());
        
        // Create mock content type detection service
        let content_type_detector = Arc::new(MockContentTypeDetector::new());
        let content_type_service = Arc::new(ContentTypeDetectionUseCase::new(content_type_detector));
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(), 
            storage.clone(), 
            publisher.clone(), 
            validator.clone(),
            Arc::new(MockVersionValidator::new()),
            content_type_service,
        );
        let endpoint = UploadArtifactEndpoint::new(Arc::new(use_case));

        let form_data = "--boundary\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{\"coordinates\":{\"namespace\":\"example\",\"name\":\"test-artifact\",\"version\":\"1.0.0\",\"qualifiers\":{}},\"file_name\":\"test.bin\"}\r\n--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.bin\"\r\nContent-Type: application/octet-stream\r\n\r\ntest content\r\n--boundary--\r\n";

        let request = Request::builder()
            .method("POST")
            .uri("/artifacts")
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(form_data))
            .unwrap();

        // Convert Request to Multipart - need to use the proper extractor configuration
        let mut extractor_config = axum::extract::DefaultBodyLimit::max(1024 * 1024); // 1MB limit
        let multipart = Multipart::from_request(request, &mut extractor_config).await.unwrap();

        // Act
        let response = UploadArtifactEndpoint::upload_artifact(Extension(Arc::new(endpoint)), multipart).await;

        // Assert - convert to Response first
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let (_parts, body) = response.into_parts();
        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        assert!(body_bytes.starts_with(b"Repository error:"));

        // Verify error logging
        assert!(logs_contain("RepositoryError"));
        assert!(logs_contain("Mock save_physical_artifact failed"));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_upload_artifact_storage_error() {
        // Arrange
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        *storage.should_fail_upload.lock().unwrap() = true;
        let publisher = Arc::new(MockEventPublisher::new());
        let validator = Arc::new(MockArtifactValidator::new());
        
        // Create mock content type detection service
        let content_type_detector = Arc::new(MockContentTypeDetector::new());
        let content_type_service = Arc::new(ContentTypeDetectionUseCase::new(content_type_detector));
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(), 
            storage.clone(), 
            publisher.clone(), 
            validator.clone(),
            Arc::new(MockVersionValidator::new()),
            content_type_service,
        );
        let endpoint = UploadArtifactEndpoint::new(Arc::new(use_case));

        let form_data = "--boundary\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{\"coordinates\":{\"namespace\":\"example\",\"name\":\"test-artifact\",\"version\":\"1.0.0\",\"qualifiers\":{}},\"file_name\":\"test.bin\"}\r\n--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.bin\"\r\nContent-Type: application/octet-stream\r\n\r\ntest content\r\n--boundary--\r\n";

        let request = Request::builder()
            .method("POST")
            .uri("/artifacts")
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(form_data))
            .unwrap();

        // Convert Request to Multipart - need to use the proper extractor configuration
        let mut extractor_config = axum::extract::DefaultBodyLimit::max(1024 * 1024); // 1MB limit
        let multipart = Multipart::from_request(request, &mut extractor_config).await.unwrap();

        // Act
        let response = UploadArtifactEndpoint::upload_artifact(Extension(Arc::new(endpoint)), multipart).await;

        // Assert - convert to Response first
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let (_parts, body) = response.into_parts();
        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        assert!(body_bytes.starts_with(b"Storage error:"));

        // Verify error logging
        assert!(logs_contain("StorageError"));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_upload_artifact_event_error() {
        // Arrange
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        *publisher.should_fail_publish.lock().unwrap() = true;
        let validator = Arc::new(MockArtifactValidator::new());
        
        // Create mock content type detection service
        let content_type_detector = Arc::new(MockContentTypeDetector::new());
        let content_type_service = Arc::new(ContentTypeDetectionUseCase::new(content_type_detector));
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(), 
            storage.clone(), 
            publisher.clone(), 
            validator.clone(),
            Arc::new(MockVersionValidator::new()),
            content_type_service,
        );
        let endpoint = UploadArtifactEndpoint::new(Arc::new(use_case));

        let form_data = "--boundary\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{\"coordinates\":{\"namespace\":\"example\",\"name\":\"test-artifact\",\"version\":\"1.0.0\",\"qualifiers\":{}},\"file_name\":\"test.bin\"}\r\n--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.bin\"\r\nContent-Type: application/octet-stream\r\n\r\ntest content\r\n--boundary--\r\n";

        let request = Request::builder()
            .method("POST")
            .uri("/artifacts")
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(form_data))
            .unwrap();

        // Convert Request to Multipart - need to use the proper extractor configuration
        let mut extractor_config = axum::extract::DefaultBodyLimit::max(1024 * 1024); // 1MB limit
        let multipart = Multipart::from_request(request, &mut extractor_config).await.unwrap();

        // Act
        let response = UploadArtifactEndpoint::upload_artifact(Extension(Arc::new(endpoint)), multipart).await;

        // Assert - convert to Response first
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let (_parts, body) = response.into_parts();
        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        assert!(body_bytes.starts_with(b"Event error:"));

        // Verify error logging
        assert!(logs_contain("EventError"));
    }

    #[tokio::test]
    #[traced_test]
    async fn test_upload_artifact_checksum_mismatch() {
        // Arrange
        let repo = Arc::new(MockArtifactRepository::new());
        let storage = Arc::new(MockArtifactStorage::new());
        let publisher = Arc::new(MockEventPublisher::new());
        let validator = Arc::new(MockArtifactValidator::new());
        
        // Create mock content type detection service
        let content_type_detector = Arc::new(MockContentTypeDetector::new());
        let content_type_service = Arc::new(ContentTypeDetectionUseCase::new(content_type_detector));
        
        let use_case = UploadArtifactUseCase::new(
            repo.clone(), 
            storage.clone(), 
            publisher.clone(), 
            validator.clone(),
            Arc::new(MockVersionValidator::new()),
            content_type_service,
        );
        let endpoint = UploadArtifactEndpoint::new(Arc::new(use_case));

        // Provide wrong checksum deliberately
        let form_data = "--boundary\r\nContent-Disposition: form-data; name=\"metadata\"\r\n\r\n{\"coordinates\":{\"namespace\":\"example\",\"name\":\"test-artifact\",\"version\":\"1.0.0\",\"qualifiers\":{}},\"file_name\":\"test.bin\",\"checksum\":\"deadbeef\"}\r\n--boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"test.bin\"\r\nContent-Type: application/octet-stream\r\n\r\ntest content\r\n--boundary--\r\n";

        let request = Request::builder()
            .method("POST")
            .uri("/artifacts")
            .header("Content-Type", "multipart/form-data; boundary=boundary")
            .body(Body::from(form_data))
            .unwrap();

        let mut extractor_config = axum::extract::DefaultBodyLimit::max(1024 * 1024);
        let multipart = Multipart::from_request(request, &mut extractor_config).await.unwrap();

        // Act
        let response = UploadArtifactEndpoint::upload_artifact(Extension(Arc::new(endpoint)), multipart).await;

        // Assert
        let response = response.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let (_parts, body) = response.into_parts();
        let body_bytes = to_bytes(body, usize::MAX).await.unwrap();
        assert_eq!(body_bytes, "Invalid checksum");
        assert!(logs_contain("Checksum mismatch"));
    }

}