use std::sync::Arc;
use axum::{http::StatusCode, Json};
use shared::hrn::Hrn;
use super::{
    api::VersioningApi,
    di::VersioningDIContainer,
    dto::{ValidateVersionCommand, VersionValidationResult, VersioningConfig},
    error::VersioningError,
    ports::{VersioningRepository, VersioningEventPublisher, VersionValidator},
    dto::ParsedVersion,
};

/// Mock implementations for testing
struct MockVersioningRepository {
    config: VersioningConfig,
    existing_versions: Vec<String>,
}

impl MockVersioningRepository {
    fn new() -> Self {
        Self {
            config: VersioningConfig::default(),
            existing_versions: vec!["1.0.0".to_string()],
        }
    }
}

#[async_trait::async_trait]
impl VersioningRepository for MockVersioningRepository {
    async fn get_versioning_config(&self, _repository_hrn: &Hrn) -> Result<VersioningConfig, VersioningError> {
        Ok(self.config.clone())
    }
    
    async fn save_versioning_config(&self, _repository_hrn: &Hrn, _config: &VersioningConfig) -> Result<(), VersioningError> {
        Ok(())
    }
    
    async fn version_exists(&self, _package_hrn: &Hrn, version: &str) -> Result<bool, VersioningError> {
        Ok(self.existing_versions.contains(&version.to_string()))
    }
    
    async fn get_existing_versions(&self, _package_hrn: &Hrn) -> Result<Vec<String>, VersioningError> {
        Ok(self.existing_versions.clone())
    }
    
    async fn snapshot_exists_for_major_minor(&self, _package_hrn: &Hrn, _major: u64, _minor: u64) -> Result<bool, VersioningError> {
        Ok(false)
    }
}

struct MockVersioningEventPublisher;

#[async_trait::async_trait]
impl VersioningEventPublisher for MockVersioningEventPublisher {
    async fn publish_version_validated(&self, _event: super::ports::VersioningEvent) -> Result<(), VersioningError> {
        Ok(())
    }
    
    async fn publish_version_validation_failed(&self, _event: super::ports::VersioningEvent) -> Result<(), VersioningError> {
        Ok(())
    }
}

struct MockVersionValidator {
    should_succeed: bool,
}

impl MockVersionValidator {
    fn new(should_succeed: bool) -> Self {
        Self { should_succeed }
    }
}

#[async_trait::async_trait]
impl VersionValidator for MockVersionValidator {
    fn parse_version(&self, version_str: &str) -> Result<ParsedVersion, VersioningError> {
        if !self.should_succeed {
            return Err(VersioningError::InvalidSemVer("Invalid version".to_string()));
        }
        
        // Simple parsing for testing
        Ok(ParsedVersion {
            original: version_str.to_string(),
            major: 1,
            minor: 0,
            patch: 0,
            prerelease: None,
            build_metadata: None,
            is_snapshot: version_str.to_lowercase().ends_with("-snapshot"),
        })
    }
    
    fn validate_version(&self, _parsed_version: &ParsedVersion, _config: &VersioningConfig) -> Result<(), VersioningError> {
        if !self.should_succeed {
            return Err(VersioningError::NotSemVerCompliant("Version not compliant".to_string()));
        }
        Ok(())
    }
}

#[tokio::test]
async fn test_validate_version_api_success() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let di_container = VersioningDIContainer::new_with_mocks(
        repository,
        event_publisher,
        version_validator,
    );
    
    let api = di_container.into_api();
    
    let request = super::api::ValidateVersionRequest {
        package_hrn: Hrn::new("hrn:artifact:test:package").unwrap(),
        version: "1.0.1".to_string(),
        repository_hrn: Hrn::new("hrn:repository:test").unwrap(),
    };
    
    // Act
    let result = api.validate_version(Json(request)).await;
    
    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.is_valid);
    assert_eq!(response.version, "1.0.1");
    assert!(response.parsed_version.is_some());
    assert!(response.errors.is_empty());
}

#[tokio::test]
async fn test_validate_version_api_failure() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(false));
    
    let di_container = VersioningDIContainer::new_with_mocks(
        repository,
        event_publisher,
        version_validator,
    );
    
    let api = di_container.into_api();
    
    let request = super::api::ValidateVersionRequest {
        package_hrn: Hrn::new("hrn:artifact:test:package").unwrap(),
        version: "invalid-version".to_string(),
        repository_hrn: Hrn::new("hrn:repository:test").unwrap(),
    };
    
    // Act
    let result = api.validate_version(Json(request)).await;
    
    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(!response.is_valid);
    assert!(response.parsed_version.is_none());
    assert_eq!(response.errors.len(), 1);
}

#[tokio::test]
async fn test_get_versioning_config_api() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let di_container = VersioningDIContainer::new_with_mocks(
        repository,
        event_publisher,
        version_validator,
    );
    
    let api = di_container.into_api();
    
    // Act
    let result = api.get_versioning_config(axum::extract::Path("hrn:repository:test".to_string())).await;
    
    // Assert
    assert!(result.is_ok());
    let config = result.unwrap();
    assert!(!config.strict_semver);
}

#[tokio::test]
async fn test_get_versioning_config_api_invalid_hrn() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let di_container = VersioningDIContainer::new_with_mocks(
        repository,
        event_publisher,
        version_validator,
    );
    
    let api = di_container.into_api();
    
    // Act
    let result = api.get_versioning_config(axum::extract::Path("invalid-hrn".to_string())).await;
    
    // Assert
    assert!(result.is_err());
    let (status, message) = result.unwrap_err();
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(message.contains("Invalid HRN format"));
}

#[tokio::test]
async fn test_update_versioning_config_api() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let di_container = VersioningDIContainer::new_with_mocks(
        repository,
        event_publisher,
        version_validator,
    );
    
    let api = di_container.into_api();
    
    let request = super::api::UpdateVersioningConfigRequest {
        config: VersioningConfig {
            strict_semver: true,
            allow_only_one_snapshot_per_major_minor: true,
            allowed_prerelease_tags: vec!["beta".to_string()],
            reject_build_metadata: true,
        },
    };
    
    // Act
    let result = api.update_versioning_config(axum::extract::Path("hrn:repository:test".to_string()), Json(request)).await;
    
    // Assert
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), StatusCode::OK);
}

#[tokio::test]
async fn test_get_existing_versions_api() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let di_container = VersioningDIContainer::new_with_mocks(
        repository,
        event_publisher,
        version_validator,
    );
    
    let api = di_container.into_api();
    
    // Act
    let result = api.get_existing_versions(axum::extract::Path("hrn:artifact:test:package".to_string())).await;
    
    // Assert
    assert!(result.is_ok());
    let versions = result.unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0], "1.0.0");
}