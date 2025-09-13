use super::api::ContentTypeDetectionApi;
use super::use_case::ContentTypeDetectionUseCase;
use super::dto::{ContentTypeDetectionResult, DetectContentTypeCommand};
use super::error::ContentTypeDetectionError;
use super::ports::{ContentTypeDetector, ContentTypeDetectionResult as PortDetectionResult};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use bytes::Bytes;
use std::sync::Arc;
use async_trait::async_trait;

// Mock del detector para testing
#[derive(Default)]
struct MockDetector;

#[async_trait]
impl ContentTypeDetector for MockDetector {
    async fn detect_from_bytes(&self, _data: Bytes) -> Result<PortDetectionResult, ContentTypeDetectionError> {
        Ok(PortDetectionResult {
            detected_mime_type: "application/pdf".to_string(),
            client_provided_mime_type: Some("application/pdf".to_string()),
            has_mismatch: false,
            confidence: 0.99,
        })
    }
    
    async fn detect_from_extension(&self, _filename: &str) -> Result<PortDetectionResult, ContentTypeDetectionError> {
        Ok(PortDetectionResult {
            detected_mime_type: "application/pdf".to_string(),
            client_provided_mime_type: Some("application/pdf".to_string()),
            has_mismatch: false,
            confidence: 0.95,
        })
    }
    
    async fn validate_consistency(
        &self, 
        detected: &str, 
        provided: Option<&str>
    ) -> Result<PortDetectionResult, ContentTypeDetectionError> {
        Ok(PortDetectionResult {
            detected_mime_type: detected.to_string(),
            client_provided_mime_type: provided.map(|s| s.to_string()),
            has_mismatch: false,
            confidence: 1.0,
        })
    }
}

#[tokio::test]
async fn test_detect_from_json_success() {
    // Mock del use_case
    let detector = Arc::new(MockDetector::default());
    let use_case = ContentTypeDetectionUseCase::new(detector);
    let api = ContentTypeDetectionApi::new(Arc::new(use_case));
    let command = DetectContentTypeCommand {
        data: b"%PDF-1.4".to_vec(),
        filename: Some("file.pdf".to_string()),
        client_content_type: Some("application/pdf".to_string()),
    };
    let response = ContentTypeDetectionApi::detect_from_json(State(api.clone()), axum::Json(command)).await.into_response();
    let (parts, _body) = response.into_parts();
    assert_eq!(parts.status, StatusCode::OK);
}