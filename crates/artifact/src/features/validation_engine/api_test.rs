use std::sync::Arc;
use axum::{http::StatusCode, Json};
use bytes::Bytes;
use shared::hrn::Hrn;
use crate::domain::package_version::PackageCoordinates;
use super::{
    api::ValidationEngineApi,
    di::ValidationEngineDIContainer,
    dto::{ValidateArtifactCommand, ValidationResult, ValidationRule, ValidationRuleType},
    error::ValidationError,
    ports::{ValidationRuleRepository, ArtifactContentReader, ValidationEventPublisher, ValidationRuleExecutor},
    dto::RuleValidationOutcome,
};

/// Mock implementations for testing
struct MockValidationRuleRepository {
    rules: Vec<ValidationRule>,
}

impl MockValidationRuleRepository {
    fn new() -> Self {
        Self {
            rules: vec![
                ValidationRule {
                    id: "test-rule".to_string(),
                    name: "Test Rule".to_string(),
                    description: "Test validation rule".to_string(),
                    enabled: true,
                    priority: 1,
                    artifact_types: vec!["test".to_string()],
                    rule_type: ValidationRuleType::SizeLimit { max_size_bytes: 1000 },
                },
            ],
        }
    }
}

#[async_trait::async_trait]
impl ValidationRuleRepository for MockValidationRuleRepository {
    async fn get_active_rules_for_artifact_type(&self, artifact_type: &str) -> Result<Vec<ValidationRule>, ValidationError> {
        if artifact_type == "test" {
            Ok(self.rules.clone())
        } else {
            Ok(Vec::new())
        }
    }
    
    async fn get_rule_by_id(&self, _rule_id: &str) -> Result<Option<ValidationRule>, ValidationError> {
        Ok(Some(self.rules[0].clone()))
    }
    
    async fn save_rule(&self, _rule: &ValidationRule) -> Result<(), ValidationError> {
        Ok(())
    }
    
    async fn delete_rule(&self, _rule_id: &str) -> Result<(), ValidationError> {
        Ok(())
    }
}

struct MockArtifactContentReader {
    content: Bytes,
}

impl MockArtifactContentReader {
    fn new(content: Bytes) -> Self {
        Self { content }
    }
}

#[async_trait::async_trait]
impl ArtifactContentReader for MockArtifactContentReader {
    async fn read_artifact_content(&self, _storage_path: &str) -> Result<Bytes, ValidationError> {
        Ok(self.content.clone())
    }
}

struct MockValidationEventPublisher;

#[async_trait::async_trait]
impl ValidationEventPublisher for MockValidationEventPublisher {
    async fn publish_validation_failed(&self, _event: crate::domain::events::ArtifactEvent) -> Result<(), ValidationError> {
        Ok(())
    }
}

struct MockValidationRuleExecutor {
    should_pass: bool,
}

impl MockValidationRuleExecutor {
    fn new(should_pass: bool) -> Self {
        Self { should_pass }
    }
}

#[async_trait::async_trait]
impl ValidationRuleExecutor for MockValidationRuleExecutor {
    async fn execute_rule(&self, rule: &ValidationRule, _context: &super::dto::ValidationContext) -> Result<RuleValidationOutcome, ValidationError> {
        Ok(RuleValidationOutcome {
            rule_id: rule.id.clone(),
            passed: self.should_pass,
            errors: if !self.should_pass { vec!["Test validation failed".to_string()] } else { Vec::new() },
            warnings: Vec::new(),
        })
    }
}

#[tokio::test]
async fn test_validate_artifact_api_success() {
    // Arrange
    let rule_repository = Arc::new(MockValidationRuleRepository::new());
    let content_reader = Arc::new(MockArtifactContentReader::new(Bytes::from("test content")));
    let event_publisher = Arc::new(MockValidationEventPublisher);
    let rule_executor = Arc::new(MockValidationRuleExecutor::new(true));
    
    let di_container = ValidationEngineDIContainer::new_with_mocks(
        content_reader,
        rule_repository,
        event_publisher,
        rule_executor,
    );
    
    let api = di_container.into_api();
    
    let request = super::api::ValidateArtifactRequest {
        package_version_hrn: Hrn::new("hrn:artifact:test:package:1.0.0").unwrap(),
        artifact_storage_path: "/test/path".to_string(),
        artifact_type: "test".to_string(),
        coordinates: PackageCoordinates {
            namespace: Some("test".to_string()),
            name: "package".to_string(),
            version: "1.0.0".to_string(),
            qualifiers: std::collections::HashMap::new(),
        },
        content_length: 500,
    };
    
    // Act
    let result = api.validate_artifact(Json(request)).await;
    
    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(response.is_valid);
    assert_eq!(response.package_version_hrn.to_string(), "hrn:artifact:test:package:1.0.0");
    assert!(response.errors.is_empty());
}

#[tokio::test]
async fn test_validate_artifact_api_failure() {
    // Arrange
    let rule_repository = Arc::new(MockValidationRuleRepository::new());
    let content_reader = Arc::new(MockArtifactContentReader::new(Bytes::from("test content")));
    let event_publisher = Arc::new(MockValidationEventPublisher);
    let rule_executor = Arc::new(MockValidationRuleExecutor::new(false));
    
    let di_container = ValidationEngineDIContainer::new_with_mocks(
        content_reader,
        rule_repository,
        event_publisher,
        rule_executor,
    );
    
    let api = di_container.into_api();
    
    let request = super::api::ValidateArtifactRequest {
        package_version_hrn: Hrn::new("hrn:artifact:test:package:1.0.0").unwrap(),
        artifact_storage_path: "/test/path".to_string(),
        artifact_type: "test".to_string(),
        coordinates: PackageCoordinates {
            namespace: Some("test".to_string()),
            name: "package".to_string(),
            version: "1.0.0".to_string(),
            qualifiers: std::collections::HashMap::new(),
        },
        content_length: 500,
    };
    
    // Act
    let result = api.validate_artifact(Json(request)).await;
    
    // Assert
    assert!(result.is_ok());
    let response = result.unwrap();
    assert!(!response.is_valid);
    assert_eq!(response.errors.len(), 1);
    assert_eq!(response.errors[0], "Test validation failed");
}

#[tokio::test]
async fn test_get_validation_rules_api() {
    // Arrange
    let rule_repository = Arc::new(MockValidationRuleRepository::new());
    let content_reader = Arc::new(MockArtifactContentReader::new(Bytes::new()));
    let event_publisher = Arc::new(MockValidationEventPublisher);
    let rule_executor = Arc::new(MockValidationRuleExecutor::new(true));
    
    let di_container = ValidationEngineDIContainer::new_with_mocks(
        content_reader,
        rule_repository,
        event_publisher,
        rule_executor,
    );
    
    let api = di_container.into_api();
    
    // Act
    let result = api.get_validation_rules("test".to_string()).await;
    
    // Assert
    assert!(result.is_ok());
    let rules = result.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "test-rule");
}

#[tokio::test]
async fn test_get_validation_rules_api_no_match() {
    // Arrange
    let rule_repository = Arc::new(MockValidationRuleRepository::new());
    let content_reader = Arc::new(MockArtifactContentReader::new(Bytes::new()));
    let event_publisher = Arc::new(MockValidationEventPublisher);
    let rule_executor = Arc::new(MockValidationRuleExecutor::new(true));
    
    let di_container = ValidationEngineDIContainer::new_with_mocks(
        content_reader,
        rule_repository,
        event_publisher,
        rule_executor,
    );
    
    let api = di_container.into_api();
    
    // Act
    let result = api.get_validation_rules("nonexistent".to_string()).await;
    
    // Assert
    assert!(result.is_ok());
    let rules = result.unwrap();
    assert!(rules.is_empty());
}

#[tokio::test]
async fn test_add_validation_rule_api() {
    // Arrange
    let rule_repository = Arc::new(MockValidationRuleRepository::new());
    let content_reader = Arc::new(MockArtifactContentReader::new(Bytes::new()));
    let event_publisher = Arc::new(MockValidationEventPublisher);
    let rule_executor = Arc::new(MockValidationRuleExecutor::new(true));
    
    let di_container = ValidationEngineDIContainer::new_with_mocks(
        content_reader,
        rule_repository,
        event_publisher,
        rule_executor,
    );
    
    let api = di_container.into_api();
    
    let request = super::api::AddValidationRuleRequest {
        id: "new-rule".to_string(),
        name: "New Rule".to_string(),
        description: "New validation rule".to_string(),
        enabled: true,
        priority: 2,
        artifact_types: vec!["test".to_string()],
        rule_type: ValidationRuleType::SizeLimit { max_size_bytes: 2000 },
    };
    
    // Act
    let result = api.add_validation_rule(Json(request)).await;
    
    // Assert
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_remove_validation_rule_api() {
    // Arrange
    let rule_repository = Arc::new(MockValidationRuleRepository::new());
    let content_reader = Arc::new(MockArtifactContentReader::new(Bytes::new()));
    let event_publisher = Arc::new(MockValidationEventPublisher);
    let rule_executor = Arc::new(MockValidationRuleExecutor::new(true));
    
    let di_container = ValidationEngineDIContainer::new_with_mocks(
        content_reader,
        rule_repository,
        event_publisher,
        rule_executor,
    );
    
    let api = di_container.into_api();
    
    // Act
    let result = api.remove_validation_rule("test-rule".to_string()).await;
    
    // Assert
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), StatusCode::NO_CONTENT);
}