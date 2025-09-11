use async_trait::async_trait;
use semver::Version;

use crate::features::upload_artifact::ports::{VersionValidator, ParsedVersion};

/// Implementación por defecto del validador de versiones usando la librería semver
#[derive(Debug, Clone)]
pub struct DefaultVersionValidator;

impl DefaultVersionValidator {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl VersionValidator for DefaultVersionValidator {
    async fn validate_version(&self, version_str: &str) -> Result<(), String> {
        // Manejar versiones SNAPSHOT (especialmente para Maven)
        let is_snapshot = version_str.to_lowercase().ends_with("-snapshot");
        let version_to_parse = if is_snapshot {
            &version_str[..version_str.len() - 9] // Remover "-snapshot"
        } else {
            version_str
        };
        
        // Parsear la versión usando la librería semver
        Version::parse(version_to_parse)
            .map(|_| ())
            .map_err(|e| format!("Invalid semantic version '{}': {}", version_str, e))
    }

    async fn parse_version(&self, version_str: &str) -> Result<ParsedVersion, String> {
        // Manejar versiones SNAPSHOT (especialmente para Maven)
        let is_snapshot = version_str.to_lowercase().ends_with("-snapshot");
        let version_to_parse = if is_snapshot {
            &version_str[..version_str.len() - 9] // Remover "-snapshot"
        } else {
            version_str
        };
        
        // Parsear la versión usando la librería semver
        let version = Version::parse(version_to_parse)
            .map_err(|e| format!("Invalid semantic version '{}': {}", version_str, e))?;
        
        Ok(ParsedVersion {
            original: version_str.to_string(),
            major: version.major,
            minor: version.minor,
            patch: version.patch,
            prerelease: if version.pre.is_empty() {
                None
            } else {
                Some(version.pre.as_str().to_string())
            },
            build_metadata: if version.build.is_empty() {
                None
            } else {
                Some(version.build.as_str().to_string())
            },
            is_snapshot,
        })
    }
}