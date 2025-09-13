use async_trait::async_trait;
use std::sync::Mutex;
use bytes::Bytes;

use super::{
    error::ContentTypeDetectionError,
    ports::{ContentTypeDetector, ContentTypeDetectionResult},
};

/// Mock para ContentTypeDetector
pub struct MockContentTypeDetector {
    pub detection_results: Mutex<Vec<ContentTypeDetectionResult>>,
    pub should_fail: Mutex<bool>,
}

impl MockContentTypeDetector {
    pub fn new() -> Self {
        Self {
            detection_results: Mutex::new(Vec::new()),
            should_fail: Mutex::new(false),
        }
    }

    pub fn with_result(result: ContentTypeDetectionResult) -> Self {
        let mut results = Vec::new();
        results.push(result);
        Self {
            detection_results: Mutex::new(results),
            should_fail: Mutex::new(false),
        }
    }

    pub fn set_should_fail(&self, should_fail: bool) {
        *self.should_fail.lock().unwrap() = should_fail;
    }

    pub fn add_result(&self, result: ContentTypeDetectionResult) {
        self.detection_results.lock().unwrap().push(result);
    }
}

#[async_trait]
impl ContentTypeDetector for MockContentTypeDetector {
    async fn detect_from_bytes(&self, data: Bytes) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        if *self.should_fail.lock().unwrap() {
            return Err(ContentTypeDetectionError::DetectionFailed);
        }

        let mut results = self.detection_results.lock().unwrap();
        if let Some(result) = results.pop() {
            Ok(result)
        } else {
            // Resultado por defecto basado en los primeros bytes
            let content_type = if data.len() > 4 {
                let header = &data[..4];
                match header {
                    b"\x50\x4B\x03\x04" => "application/zip",
                    b"\x1F\x8B\x08\x00" => "application/gzip",
                    _ => "application/octet-stream",
                }
            } else {
                "application/octet-stream"
            }.to_string();

            Ok(ContentTypeDetectionResult {
                detected_mime_type: content_type,
                client_provided_mime_type: None,
                has_mismatch: false,
                confidence: 0.95,
            })
        }
    }

    async fn detect_from_extension(&self, filename: &str) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        if *self.should_fail.lock().unwrap() {
            return Err(ContentTypeDetectionError::DetectionFailed);
        }

        let extension = filename.split('.').last().unwrap_or("");
        let content_type = match extension {
            "jar" | "war" => "application/java-archive",
            "zip" => "application/zip",
            "tar" => "application/x-tar",
            "gz" => "application/gzip",
            "xml" => "application/xml",
            "json" => "application/json",
            "js" => "application/javascript",
            "ts" => "application/typescript",
            "py" => "text/x-python",
            "rb" => "text/x-ruby",
            "go" => "text/x-go",
            "rs" => "text/x-rust",
            "c" | "h" => "text/x-c",
            "cpp" | "hpp" => "text/x-c++",
            "md" => "text/markdown",
            "txt" => "text/plain",
            "yaml" | "yml" => "application/x-yaml",
            "toml" => "application/toml",
            "dockerfile" => "text/x-dockerfile",
            "sh" => "text/x-shellscript",
            "bat" => "application/x-bat",
            "ps1" => "application/x-powershell",
            _ => "application/octet-stream",
        }.to_string();

        Ok(ContentTypeDetectionResult {
            detected_mime_type: content_type,
            client_provided_mime_type: None,
            has_mismatch: false,
            confidence: 0.85,
        })
    }

    async fn validate_consistency(
        &self,
        detected: &str,
        provided: Option<&str>
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        if *self.should_fail.lock().unwrap() {
            return Err(ContentTypeDetectionError::DetectionFailed);
        }

        let has_mismatch = if let Some(provided_type) = provided {
            detected != provided_type
        } else {
            false
        };

        Ok(ContentTypeDetectionResult {
            detected_mime_type: detected.to_string(),
            client_provided_mime_type: provided.map(|s| s.to_string()),
            has_mismatch,
            confidence: if has_mismatch { 0.7 } else { 0.95 },
        })
    }
}
