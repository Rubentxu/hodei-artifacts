use std::sync::Arc;
use shared::hrn::Hrn;
use super::{
    use_case::VersioningUseCase,
    dto::{ValidateVersionCommand, VersionValidationResult, VersioningConfig},
    error::VersioningError,
    ports::{VersioningRepository, VersioningEventPublisher, VersionValidator},
    dto::ParsedVersion,
};

/// Mock implementation for testing
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
    
    fn with_config(config: VersioningConfig) -> Self {
        Self {
            config,
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

/// Mock implementation for testing
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

/// Mock implementation for testing
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
async fn test_validate_version_success() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let use_case = VersioningUseCase::new(
        repository,
        event_publisher,
        version_validator,
    );
    
    let command = ValidateVersionCommand {
        package_hrn: Hrn::new("hrn:artifact:test:package").unwrap(),
        version: "1.0.0".to_string(),
        repository_hrn: Hrn::new("hrn:repository:test").unwrap(),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(validation_result.is_valid);
    assert_eq!(validation_result.version, "1.0.0");
    assert!(validation_result.parsed_version.is_some());
    assert!(validation_result.errors.is_empty());
}

#[tokio::test]
async fn test_validate_version_already_exists() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let use_case = VersioningUseCase::new(
        repository,
        event_publisher,
        version_validator,
    );
    
    let command = ValidateVersionCommand {
        package_hrn: Hrn::new("hrn:artifact:test:package").unwrap(),
        version: "1.0.0".to_string(), // This version exists in the mock
        repository_hrn: Hrn::new("hrn:repository:test").unwrap(),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(!validation_result.is_valid);
    assert_eq!(validation_result.errors.len(), 1);
    assert!(validation_result.errors[0].contains("already exists"));
}

#[tokio::test]
async fn test_validate_version_parse_failure() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(false));
    
    let use_case = VersioningUseCase::new(
        repository,
        event_publisher,
        version_validator,
    );
    
    let command = ValidateVersionCommand {
        package_hrn: Hrn::new("hrn:artifact:test:package").unwrap(),
        version: "invalid-version".to_string(),
        repository_hrn: Hrn::new("hrn:repository:test").unwrap(),
    };
    
    // Act
    let result = use_case.execute(command).await;
    
    // Assert
    assert!(result.is_ok());
    let validation_result = result.unwrap();
    assert!(!validation_result.is_valid);
    assert!(validation_result.parsed_version.is_none());
    assert_eq!(validation_result.errors.len(), 1);
}

#[tokio::test]
async fn test_get_versioning_config() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let use_case = VersioningUseCase::new(
        repository,
        event_publisher,
        version_validator,
    );
    
    let repository_hrn = Hrn::new("hrn:repository:test").unwrap();
    
    // Act
    let result = use_case.get_versioning_config(&repository_hrn).await;
    
    // Assert
    assert!(result.is_ok());
    let config = result.unwrap();
    assert!(!config.strict_semver);
}

#[tokio::test]
async fn test_update_versioning_config() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let use_case = VersioningUseCase::new(
        repository,
        event_publisher,
        version_validator,
    );
    
    let repository_hrn = Hrn::new("hrn:repository:test").unwrap();
    let config = VersioningConfig {
        strict_semver: true,
        allow_only_one_snapshot_per_major_minor: true,
        allowed_prerelease_tags: vec!["beta".to_string()],
        reject_build_metadata: true,
    };
    
    // Act
    let result = use_case.update_versioning_config(&repository_hrn, &config).await;
    
    // Assert
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_get_existing_versions() {
    // Arrange
    let repository = Arc::new(MockVersioningRepository::new());
    let event_publisher = Arc::new(MockVersioningEventPublisher);
    let version_validator = Arc::new(MockVersionValidator::new(true));
    
    let use_case = VersioningUseCase::new(
        repository,
        event_publisher,
        version_validator,
    );
    
    let package_hrn = Hrn::new("hrn:artifact:test:package").unwrap();
    
    // Act
    let result = use_case.get_existing_versions(&package_hrn).await;
    
    // Assert
    assert!(result.is_ok());
    let versions = result.unwrap();
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0], "1.0.0");
}