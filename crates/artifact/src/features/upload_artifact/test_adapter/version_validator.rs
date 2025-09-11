use async_trait::async_trait;

use crate::features::upload_artifact::ports::{VersionValidator, ParsedVersion};

/// Mock implementation del validador de versiones para testing
#[derive(Debug, Clone)]
pub struct MockVersionValidator {
    pub should_fail: bool,
    pub error_message: Option<String>,
}

impl MockVersionValidator {
    pub fn new() -> Self {
        Self {
            should_fail: false,
            error_message: None,
        }
    }
    
    pub fn with_failure(mut self, error_message: &str) -> Self {
        self.should_fail = true;
        self.error_message = Some(error_message.to_string());
        self
    }
}

#[async_trait]
impl VersionValidator for MockVersionValidator {
    async fn validate_version(&self, version_str: &str) -> Result<(), String> {
        if self.should_fail {
            return Err(self.error_message.clone().unwrap_or_else(|| "Mock validation failed".to_string()));
        }
        
        // Validación básica para testing
        if version_str.is_empty() {
            return Err("Version cannot be empty".to_string());
        }
        
        Ok(())
    }

    async fn parse_version(&self, version_str: &str) -> Result<ParsedVersion, String> {
        if self.should_fail {
            return Err(self.error_message.clone().unwrap_or_else(|| "Mock parsing failed".to_string()));
        }
        
        // Parseo básico para testing
        let parts: Vec<&str> = version_str.split('.').collect();
        if parts.len() < 3 {
            return Err("Version must have at least 3 parts".to_string());
        }
        
        let major = parts[0].parse().unwrap_or(1);
        let minor = parts[1].parse().unwrap_or(0);
        let patch = parts[2].parse().unwrap_or(0);
        
        let is_snapshot = version_str.to_lowercase().ends_with("-snapshot");
        
        Ok(ParsedVersion {
            original: version_str.to_string(),
            major,
            minor,
            patch,
            prerelease: None,
            build_metadata: None,
            is_snapshot,
        })
    }
}