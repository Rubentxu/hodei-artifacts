use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use shared::hrn::Hrn;
use crate::domain::package_version::{PackageCoordinates, ArtifactDependency};
use crate::domain::events::ArtifactEvent;
use super::{
    dto::{ValidateArtifactCommand, ValidationResult, ValidationRule, ValidationContext, RuleValidationOutcome},
    error::ValidationError,
};

// Define a type alias for the Result type used in ports
pub type PortResult<T> = Result<T, ValidationError>;

/// Repository port for validation rules
#[async_trait]
pub trait ValidationRuleRepository: Send + Sync {
    /// Get all active validation rules for a specific artifact type
    async fn get_active_rules_for_artifact_type(&self, artifact_type: &str) -> PortResult<Vec<ValidationRule>>;
    
    /// Get a specific validation rule by ID
    async fn get_rule_by_id(&self, rule_id: &str) -> PortResult<Option<ValidationRule>>;
    
    /// Save a validation rule
    async fn save_rule(&self, rule: &ValidationRule) -> PortResult<()>;
    
    /// Delete a validation rule by ID
    async fn delete_rule(&self, rule_id: &str) -> PortResult<()>;
}

/// Port for reading artifact content from storage
#[async_trait]
pub trait ArtifactContentReader: Send + Sync {
    /// Read artifact content from storage
    async fn read_artifact_content(&self, storage_path: &str) -> PortResult<Bytes>;
}

/// Port for publishing validation events
#[async_trait]
pub trait ValidationEventPublisher: Send + Sync {
    /// Publish ArtifactValidationFailed event
    async fn publish_validation_failed(&self, event: ArtifactEvent) -> PortResult<()>;
}

/// Port for validation rule executors
#[async_trait]
pub trait ValidationRuleExecutor: Send + Sync {
    /// Execute a validation rule against an artifact
    async fn execute_rule(&self, rule: &ValidationRule, context: &ValidationContext) -> PortResult<RuleValidationOutcome>;
}