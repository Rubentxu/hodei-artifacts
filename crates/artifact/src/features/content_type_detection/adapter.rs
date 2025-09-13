//! Adaptadores concretos para detección de Content-Type
//! Implementación principal usando la crate `infer` para magic numbers

use super::ports::{ContentTypeDetector, ContentTypeDetectionResult};
use super::error::ContentTypeDetectionError;
use async_trait::async_trait;
use bytes::Bytes;
use infer::Infer;
use std::path::Path;
use tracing::{info, warn, debug};

/// Adaptador principal que usa la crate `infer` para detección de magic numbers
#[derive(Default)]
pub struct InferContentTypeDetector {
    infer: Infer,
}

impl InferContentTypeDetector {
    pub fn new() -> Self {
        Self {
            infer: Infer::new(),
        }
    }
    
    /// Detecta MIME type basado en extensión de archivo (fallback)
    fn detect_from_extension_internal(&self, filename: &str) -> Option<String> {
        let path = Path::new(filename);
        if let Some(extension) = path.extension().and_then(|ext| ext.to_str()) {
            match extension.to_lowercase().as_str() {
                "jar" | "war" | "ear" => Some("application/java-archive".to_string()),
                "zip" => Some("application/zip".to_string()),
                "tar" => Some("application/x-tar".to_string()),
                "gz" | "tgz" => Some("application/gzip".to_string()),
                "pdf" => Some("application/pdf".to_string()),
                "png" => Some("image/png".to_string()),
                "jpg" | "jpeg" => Some("image/jpeg".to_string()),
                "gif" => Some("image/gif".to_string()),
                "txt" => Some("text/plain".to_string()),
                "json" => Some("application/json".to_string()),
                "xml" => Some("application/xml".to_string()),
                "html" | "htm" => Some("text/html".to_string()),
                "js" => Some("application/javascript".to_string()),
                "css" => Some("text/css".to_string()),
                _ => None,
            }
        } else {
            None
        }
    }
}

#[async_trait]
impl ContentTypeDetector for InferContentTypeDetector {
    async fn detect_from_bytes(&self, data: Bytes) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        if data.len() < 256 {
            warn!(data_len = data.len(), "Datos insuficientes para detección confiable de Content-Type");
            return Err(ContentTypeDetectionError::InsufficientData(256));
        }
        
        match self.infer.get(&data) {
            Some(info) => {
                let mime_type = info.mime_type();
                debug!(mime_type = %mime_type, "Content-Type detectado mediante magic numbers");
                
                Ok(ContentTypeDetectionResult {
                    detected_mime_type: mime_type.to_string(),
                    client_provided_mime_type: None,
                    has_mismatch: false,
                    confidence: 0.9, // Alta confianza en detección por magic numbers
                })
            }
            None => {
                warn!("No se pudo detectar Content-Type mediante magic numbers");
                Err(ContentTypeDetectionError::DetectionFailed)
            }
        }
    }
    
    async fn detect_from_extension(&self, filename: &str) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        match self.detect_from_extension_internal(filename) {
            Some(mime_type) => {
                debug!(filename = %filename, mime_type = %mime_type, "Content-Type detectado mediante extensión");
                
                Ok(ContentTypeDetectionResult {
                    detected_mime_type: mime_type,
                    client_provided_mime_type: None,
                    has_mismatch: false,
                    confidence: 0.6, // Media confianza en detección por extensión
                })
            }
            None => {
                warn!(filename = %filename, "No se pudo detectar Content-Type mediante extensión");
                Err(ContentTypeDetectionError::DetectionFailed)
            }
        }
    }
    
    async fn validate_consistency(
        &self, 
        detected: &str, 
        provided: Option<&str>
    ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
        let has_mismatch = match provided {
            Some(p) => detected != p,
            None => false,
        };
        
        if has_mismatch {
            warn!(
                detected = %detected, 
                provided = ?provided, 
                "Discrepancia detectada entre Content-Type proporcionado y detectado"
            );
        } else if provided.is_some() {
            info!(
                detected = %detected, 
                provided = ?provided, 
                "Content-Type consistente entre proporcionado y detectado"
            );
        }
        
        Ok(ContentTypeDetectionResult {
            detected_mime_type: detected.to_string(),
            client_provided_mime_type: provided.map(|s| s.to_string()),
            has_mismatch,
            confidence: if has_mismatch { 0.95 } else { 1.0 }, // Mayor confianza cuando hay match
        })
    }
}

/// Adaptadores para testing
#[cfg(test)]
pub mod test {
    use super::*;
    
    #[derive(Default)]
    pub struct MockContentTypeDetector {
        pub fixed_result: Option<Result<ContentTypeDetectionResult, ContentTypeDetectionError>>,
    }
    
    #[async_trait]
    impl ContentTypeDetector for MockContentTypeDetector {
        async fn detect_from_bytes(&self, _data: Bytes) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
            match &self.fixed_result {
                Some(Ok(result)) => Ok(result.clone()),
                Some(Err(error)) => Err(error.clone()),
                None => Err(ContentTypeDetectionError::DetectionFailed),
            }
        }
        
        async fn detect_from_extension(&self, _filename: &str) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
            match &self.fixed_result {
                Some(Ok(result)) => Ok(result.clone()),
                Some(Err(error)) => Err(error.clone()),
                None => Err(ContentTypeDetectionError::DetectionFailed),
            }
        }
        
        async fn validate_consistency(
            &self, 
            detected: &str, 
            provided: Option<&str>
        ) -> Result<ContentTypeDetectionResult, ContentTypeDetectionError> {
            let has_mismatch = match provided {
                Some(p) => detected != p,
                None => false,
            };
            
            Ok(ContentTypeDetectionResult {
                detected_mime_type: detected.to_string(),
                client_provided_mime_type: provided.map(|s| s.to_string()),
                has_mismatch,
                confidence: 1.0,
            })
        }
    }
}