use async_trait::async_trait;
use std::collections::HashMap;
use shared::hrn::Hrn;
use super::{
    dto::{VersioningConfig, ParsedVersion, ValidateVersionCommand, VersionValidationResult},
    error::VersioningError,
};

// Define a type alias for the Result type used in ports
pub type PortResult<T> = Result<T, VersioningError>;

/// Repository port for versioning configuration and existing versions
#[async_trait]
pub trait VersioningRepository: Send + Sync {
    /// Get versioning configuration for a repository
    async fn get_versioning_config(&self, repository_hrn: &Hrn) -> PortResult<VersioningConfig>;
    
    /// Save versioning configuration for a repository
    async fn save_versioning_config(&self, repository_hrn: &Hrn, config: &VersioningConfig) -> PortResult<()>;
    
    /// Check if a version already exists for a package
    async fn version_exists(&self, package_hrn: &Hrn, version: &str) -> PortResult<bool>;
    
    /// Get all existing versions for a package
    async fn get_existing_versions(&self, package_hrn: &Hrn) -> PortResult<Vec<String>>;
    
    /// Check if a snapshot version exists for major.minor combination
    async fn snapshot_exists_for_major_minor(&self, package_hrn: &Hrn, major: u64, minor: u64) -> PortResult<bool>;
}

/// Port for publishing version validation events
#[async_trait]
pub trait VersioningEventPublisher: Send + Sync {
    /// Publish version validated event
    async fn publish_version_validated(&self, event: VersioningEvent) -> PortResult<()>;
    
    /// Publish version validation failed event
    async fn publish_version_validation_failed(&self, event: VersioningEvent) -> PortResult<()>;
}

/// Port for version validation logic
#[async_trait]
pub trait VersionValidator: Send + Sync {
    /// Parse and validate a version string
    fn parse_version(&self, version_str: &str) -> PortResult<ParsedVersion>;
    
    /// Validate a parsed version against policies
    fn validate_version(&self, parsed_version: &ParsedVersion, config: &VersioningConfig) -> PortResult<()>;
}

/// Versioning events
#[derive(Debug, Clone)]
pub enum VersioningEvent {
    /// Event for successful version validation
    VersionValidated {
        package_hrn: Hrn,
        version: String,
        validated_at: time::OffsetDateTime,
    },
    
    /// Event for failed version validation
    VersionValidationFailed {
        package_hrn: Hrn,
        version: String,
        errors: Vec<String>,
        failed_at: time::OffsetDateTime,
    },
}