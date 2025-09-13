use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use tracing::{debug, warn};
use semver::Version;
use super::{
    dto::{VersioningConfig, ParsedVersion},
    error::VersioningError,
    ports::{VersioningRepository, VersioningEventPublisher, VersionValidator},
};

/// Adapter for versioning configuration and version storage
pub struct RepositoryVersioningAdapter {
    // In a real implementation, this would hold a reference to the repository
    configs: HashMap<String, VersioningConfig>,
    existing_versions: HashMap<String, Vec<String>>,
}

impl RepositoryVersioningAdapter {
    pub fn new() -> Self {
        let mut configs = HashMap::new();
        let mut existing_versions = HashMap::new();
        
        // Add some default configurations
        configs.insert("default".to_string(), VersioningConfig::default());
        
        Self {
            configs,
            existing_versions,
        }
    }
}

#[async_trait]
impl VersioningRepository for RepositoryVersioningAdapter {
    async fn get_versioning_config(&self, repository_hrn: &shared::hrn::Hrn) -> Result<VersioningConfig, VersioningError> {
        debug!("Getting versioning config for repository: {}", repository_hrn);
        
        // Try to get config for this specific repository
        let repo_key = repository_hrn.to_string();
        if let Some(config) = self.configs.get(&repo_key) {
            return Ok(config.clone());
        }
        
        // Fall back to default config
        if let Some(config) = self.configs.get("default") {
            Ok(config.clone())
        } else {
            Ok(VersioningConfig::default())
        }
    }
    
    async fn save_versioning_config(&self, repository_hrn: &shared::hrn::Hrn, config: &VersioningConfig) -> Result<(), VersioningError> {
        debug!("Saving versioning config for repository: {}", repository_hrn);
        // In a real implementation, this would save to the database
        Ok(())
    }
    
    async fn version_exists(&self, package_hrn: &shared::hrn::Hrn, version: &str) -> Result<bool, VersioningError> {
        debug!("Checking if version {} exists for package {}", version, package_hrn);
        
        let package_key = package_hrn.to_string();
        if let Some(versions) = self.existing_versions.get(&package_key) {
            Ok(versions.contains(&version.to_string()))
        } else {
            Ok(false)
        }
    }
    
    async fn get_existing_versions(&self, package_hrn: &shared::hrn::Hrn) -> Result<Vec<String>, VersioningError> {
        debug!("Getting existing versions for package: {}", package_hrn);
        
        let package_key = package_hrn.to_string();
        if let Some(versions) = self.existing_versions.get(&package_key) {
            Ok(versions.clone())
        } else {
            Ok(Vec::new())
        }
    }
    
    async fn snapshot_exists_for_major_minor(&self, package_hrn: &shared::hrn::Hrn, major: u64, minor: u64) -> Result<bool, VersioningError> {
        debug!("Checking if snapshot exists for {}.{} in package {}", major, minor, package_hrn);
        
        let package_key = package_hrn.to_string();
        if let Some(versions) = self.existing_versions.get(&package_key) {
            for version in versions {
                if version.ends_with("-SNAPSHOT") {
                    // Parse the version to check major.minor
                    let version_without_snapshot = &version[..version.len() - 9];
                    if let Ok(parsed) = Version::parse(version_without_snapshot) {
                        if parsed.major == major && parsed.minor == minor {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        
        Ok(false)
    }
}

/// Adapter for publishing versioning events
pub struct EventBusVersioningPublisher {
    // In a real implementation, this would hold a reference to the event publisher
}

impl EventBusVersioningPublisher {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl VersioningEventPublisher for EventBusVersioningPublisher {
    async fn publish_version_validated(&self, event: super::ports::VersioningEvent) -> Result<(), VersioningError> {
        debug!("Publishing version validated event");
        // In a real implementation, this would publish the event to the event bus
        Ok(())
    }
    
    async fn publish_version_validation_failed(&self, event: super::ports::VersioningEvent) -> Result<(), VersioningError> {
        debug!("Publishing version validation failed event");
        // In a real implementation, this would publish the event to the event bus
        Ok(())
    }
}

/// Adapter for version validation logic
pub struct SemverVersionValidator {
    config: VersioningConfig,
}

impl SemverVersionValidator {
    pub fn new(config: VersioningConfig) -> Self {
        Self { config }
    }
    
    pub fn default() -> Self {
        Self::new(VersioningConfig::default())
    }
}

#[async_trait]
impl VersionValidator for SemverVersionValidator {
    fn parse_version(&self, version_str: &str) -> Result<ParsedVersion, VersioningError> {
        debug!("Parsing version: {}", version_str);
        
        // Handle SNAPSHOT versions (especially for Maven)
        let is_snapshot = version_str.to_lowercase().ends_with("-snapshot");
        let version_to_parse = if is_snapshot {
            &version_str[..version_str.len() - 9] // Remove "-snapshot"
        } else {
            version_str
        };
        
        // Parse the version using the semver library
        let version = Version::parse(version_to_parse)
            .map_err(|e| VersioningError::InvalidSemVer(format!("{}: {}", version_str, e)))?;
        
        // If strict SemVer is required, verify no non-standard parts
        if self.config.strict_semver {
            // The semver library already follows SemVer 2.0.0, but we can make
            // additional verifications if needed
        }
        
        let parsed_version = ParsedVersion {
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
        };
        
        Ok(parsed_version)
    }
    
    fn validate_version(&self, parsed_version: &ParsedVersion, config: &VersioningConfig) -> Result<(), VersioningError> {
        debug!("Validating version: {}", parsed_version.original);
        
        // Validate SNAPSHOT policy
        if parsed_version.is_snapshot && config.allow_only_one_snapshot_per_major_minor {
            // This validation would require database access to verify
            // if a SNAPSHOT version already exists for the same major.minor
            // We'll handle this in the use case
        }
        
        // Validate allowed pre-release tags
        if let Some(ref prerelease) = parsed_version.prerelease {
            if !config.allowed_prerelease_tags.is_empty() 
                && !config.allowed_prerelease_tags.iter().any(|tag| prerelease.contains(tag)) {
                return Err(VersioningError::PrereleaseTagNotAllowed(prerelease.clone()));
            }
        }
        
        // Validate build metadata
        if parsed_version.build_metadata.is_some() && config.reject_build_metadata {
            return Err(VersioningError::BuildMetadataNotAllowed(
                parsed_version.build_metadata.clone().unwrap_or_default()
            ));
        }
        
        Ok(())
    }
}