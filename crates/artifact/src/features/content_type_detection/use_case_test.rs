use super::error::ContentTypeDetectionError;
use super::ports::{ContentTypeDetectionResult, ContentTypeDetector};
use super::use_case::ContentTypeDetectionUseCase;
use async_trait::async_trait;
use bytes::Bytes;
use std::sync::Arc;

pub struct MockDetector;

#[async_trait]
impl ContentTypeDetector for MockDetector {
    async fn detect_from_bytes(
        &self,
        _data: Bytes,
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        Ok(ContentTypeDetectionResult {
            detected_mime_type: "application/pdf".to_string(),
            client_provided_mime_type: Some("application/pdf".to_string()),
            has_mismatch: false,
            confidence: 0.99,
        })
    }
    async fn detect_from_extension(
        &self,
        _filename: &str,
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        Ok(ContentTypeDetectionResult {
            detected_mime_type: "application/pdf".to_string(),
            client_provided_mime_type: Some("application/pdf".to_string()),
            has_mismatch: false,
            confidence: 0.95,
        })
    }
    async fn validate_consistency(
        &self,
        detected: &str,
        provided: Option<&str>,
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        Ok(ContentTypeDetectionResult {
            detected_mime_type: detected.to_string(),
            client_provided_mime_type: provided.map(|s| s.to_string()),
            has_mismatch: false,
            confidence: 1.0,
        })
    }
}

#[tokio::test]
async fn test_detect_content_type_success() {
    let detector = Arc::new(MockDetector);
    let use_case = ContentTypeDetectionUseCase::new(detector);
    let data = Bytes::from_static(b"%PDF-1.4");
    let result = use_case
        .detect_content_type(data, Some("file.pdf"), Some("application/pdf"))
        .await;
    assert!(result.is_ok());
    let res = result.unwrap();
    assert_eq!(res.detected_mime_type, "application/pdf");
    assert!(!res.has_mismatch);
}
