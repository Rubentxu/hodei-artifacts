use std::sync::Arc;
use tracing::{info, warn, error};
use time::OffsetDateTime;
use super::{
    dto::{ValidateVersionCommand, VersionValidationResult, VersioningConfig},
    error::VersioningError,
    ports::{VersioningRepository, VersioningEventPublisher, VersionValidator},
};

/// Use case for version validation and management
#[derive(Clone)]
pub struct VersioningUseCase {
    repository: Arc<dyn VersioningRepository>,
    event_publisher: Arc<dyn VersioningEventPublisher>,
    version_validator: Arc<dyn VersionValidator>,
}

impl VersioningUseCase {
    pub fn new(
        repository: Arc<dyn VersioningRepository>,
        event_publisher: Arc<dyn VersioningEventPublisher>,
        version_validator: Arc<dyn VersionValidator>,
    ) -> Self {
        Self {
            repository,
            event_publisher,
            version_validator,
        }
    }
    
    /// Execute version validation process
    pub async fn execute(&self, command: ValidateVersionCommand) -> Result<VersionValidationResult, VersioningError> {
        info!("Validating version {} for package {}", command.version, command.package_hrn);
        
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        
        // Get versioning configuration for the repository
        let config = self.repository
            .get_versioning_config(&command.repository_hrn)
            .await
            .map_err(|e| {
                error!("Failed to get versioning config: {}", e);
                VersioningError::RepositoryConfigError(e.to_string())
            })?;
        
        // Parse the version
        let parsed_version = match self.version_validator.parse_version(&command.version) {
            Ok(parsed) => parsed,
            Err(e) => {
                let error_msg = format!("Failed to parse version {}: {}", command.version, e);
                errors.push(error_msg.clone());
                
                // Publish validation failed event
                self.publish_validation_failed_event(
                    &command.package_hrn,
                    &command.version,
                    vec![error_msg],
                ).await;
                
                return Ok(VersionValidationResult {
                    package_hrn: command.package_hrn,
                    version: command.version,
                    is_valid: false,
                    parsed_version: None,
                    errors,
                    warnings,
                });
            }
        };
        
        // Validate the version against policies
        if let Err(e) = self.version_validator.validate_version(&parsed_version, &config) {
            errors.push(e.to_string());
        }
        
        // Check if version already exists
        if self.repository.version_exists(&command.package_hrn, &command.version).await? {
            errors.push(format!("Version {} already exists for package {}", command.version, command.package_hrn));
        }
        
        // Check snapshot policy if applicable
        if parsed_version.is_snapshot && config.allow_only_one_snapshot_per_major_minor {
            if self.repository.snapshot_exists_for_major_minor(
                &command.package_hrn, 
                parsed_version.major, 
                parsed_version.minor
            ).await? {
                errors.push(format!("Snapshot version already exists for {}.{}", parsed_version.major, parsed_version.minor));
            }
        }
        
        let is_valid = errors.is_empty();
        
        // Publish appropriate event
        if is_valid {
            self.publish_validation_success_event(&command.package_hrn, &command.version).await;
        } else {
            self.publish_validation_failed_event(
                &command.package_hrn,
                &command.version,
                errors.clone(),
            ).await;
        }
        
        info!("Version validation completed for {}: {}", command.package_hrn, 
              if is_valid { "PASSED" } else { "FAILED" });
        
        Ok(VersionValidationResult {
            package_hrn: command.package_hrn,
            version: command.version,
            is_valid,
            parsed_version: Some(parsed_version),
            errors,
            warnings,
        })
    }
    
    /// Get versioning configuration for a repository
    pub async fn get_versioning_config(&self, repository_hrn: &shared::hrn::Hrn) -> Result<VersioningConfig, VersioningError> {
        self.repository
            .get_versioning_config(repository_hrn)
            .await
    }
    
    /// Update versioning configuration for a repository
    pub async fn update_versioning_config(
        &self, 
        repository_hrn: &shared::hrn::Hrn, 
        config: &VersioningConfig
    ) -> Result<(), VersioningError> {
        info!("Updating versioning config for repository {}", repository_hrn);
        self.repository
            .save_versioning_config(repository_hrn, config)
            .await
    }
    
    /// Get existing versions for a package
    pub async fn get_existing_versions(&self, package_hrn: &shared::hrn::Hrn) -> Result<Vec<String>, VersioningError> {
        self.repository
            .get_existing_versions(package_hrn)
            .await
    }
    
    /// Publish validation success event
    async fn publish_validation_success_event(&self, package_hrn: &shared::hrn::Hrn, version: &str) {
        let event = super::ports::VersioningEvent::VersionValidated {
            package_hrn: package_hrn.clone(),
            version: version.to_string(),
            validated_at: OffsetDateTime::now_utc(),
        };
        
        if let Err(e) = self.event_publisher.publish_version_validated(event).await {
            error!("Failed to publish version validated event: {}", e);
        }
    }
    
    /// Publish validation failed event
    async fn publish_validation_failed_event(&self, package_hrn: &shared::hrn::Hrn, version: &str, errors: Vec<String>) {
        let event = super::ports::VersioningEvent::VersionValidationFailed {
            package_hrn: package_hrn.clone(),
            version: version.to_string(),
            errors,
            failed_at: OffsetDateTime::now_utc(),
        };
        
        if let Err(e) = self.event_publisher.publish_version_validation_failed(event).await {
            error!("Failed to publish version validation failed event: {}", e);
        }
    }
}