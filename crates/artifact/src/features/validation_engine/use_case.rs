use super::{
    dto::{
        RuleValidationOutcome, ValidateArtifactCommand, ValidationContext, ValidationResult,
        ValidationRule,
    },
    error::ValidationError,
    ports::{
        ArtifactContentReader, ValidationEventPublisher, ValidationRuleExecutor,
        ValidationRuleRepository,
    },
};
use crate::domain::events::ArtifactEvent;
use std::sync::Arc;
use time::OffsetDateTime;
use tracing::{error, info, warn};

/// Use case for validating artifacts against configurable rules
#[derive(Clone)]
pub struct ValidationEngineUseCase {
    rule_repository: Arc<dyn ValidationRuleRepository>,
    content_reader: Arc<dyn ArtifactContentReader>,
    event_publisher: Arc<dyn ValidationEventPublisher>,
    rule_executor: Arc<dyn ValidationRuleExecutor>,
}

impl ValidationEngineUseCase {
    pub fn new(
        rule_repository: Arc<dyn ValidationRuleRepository>,
        content_reader: Arc<dyn ArtifactContentReader>,
        event_publisher: Arc<dyn ValidationEventPublisher>,
        rule_executor: Arc<dyn ValidationRuleExecutor>,
    ) -> Self {
        Self {
            rule_repository,
            content_reader,
            event_publisher,
            rule_executor,
        }
    }

    /// Execute validation process for an artifact
    pub async fn execute(
        &self,
        command: ValidateArtifactCommand,
    ) -> Result<ValidationResult, ValidationError> {
        info!("Validating artifact: {}", command.package_version_hrn);

        // Read artifact content
        let artifact_content = self
            .content_reader
            .read_artifact_content(&command.artifact_storage_path)
            .await
            .map_err(|e| {
                error!("Failed to read artifact content: {}", e);
                ValidationError::StorageError(e.to_string())
            })?;

        // Get active validation rules for this artifact type
        let rules = self
            .rule_repository
            .get_active_rules_for_artifact_type(&command.artifact_type)
            .await
            .map_err(|e| {
                error!("Failed to get validation rules: {}", e);
                ValidationError::RepositoryError(e.to_string())
            })?;

        // Create validation context
        let context = ValidationContext {
            artifact_content: artifact_content.clone(),
            artifact_storage_path: command.artifact_storage_path.clone(),
            artifact_type: command.artifact_type.clone(),
            coordinates: command.coordinates.clone(),
            content_length: command.content_length,
        };

        // Execute validation rules
        let mut all_errors = Vec::new();
        let mut all_warnings = Vec::new();
        let mut overall_valid = true;

        for rule in rules {
            info!(
                "Executing validation rule: {} (priority: {})",
                rule.name, rule.priority
            );

            match self.rule_executor.execute_rule(&rule, &context).await {
                Ok(outcome) => {
                    if !outcome.passed {
                        overall_valid = false;
                        all_errors.extend(outcome.errors);
                    }
                    all_warnings.extend(outcome.warnings);
                }
                Err(e) => {
                    warn!("Validation rule {} failed to execute: {}", rule.name, e);
                    all_errors.push(format!("Rule execution failed for {}: {}", rule.name, e));
                    overall_valid = false;
                }
            }
        }

        // Publish validation event if failed
        if !overall_valid {
            let event = ArtifactEvent::ArtifactValidationFailed(
                crate::domain::events::ArtifactValidationFailed {
                    coordinates: command.coordinates.clone(),
                    errors: all_errors.clone(),
                    at: OffsetDateTime::now_utc(),
                },
            );

            if let Err(e) = self.event_publisher.publish_validation_failed(event).await {
                error!("Failed to publish validation failed event: {}", e);
            }
        }

        info!(
            "Validation completed for {}: {}",
            command.package_version_hrn,
            if overall_valid { "PASSED" } else { "FAILED" }
        );

        Ok(ValidationResult {
            package_version_hrn: command.package_version_hrn,
            is_valid: overall_valid,
            errors: all_errors,
            warnings: all_warnings,
        })
    }

    /// Get all active validation rules for an artifact type
    pub async fn get_active_rules(
        &self,
        artifact_type: &str,
    ) -> Result<Vec<ValidationRule>, ValidationError> {
        self.rule_repository
            .get_active_rules_for_artifact_type(artifact_type)
            .await
            .map_err(|e| ValidationError::RepositoryError(e.to_string()))
    }

    /// Add a new validation rule
    pub async fn add_validation_rule(&self, rule: &ValidationRule) -> Result<(), ValidationError> {
        info!("Adding validation rule: {}", rule.name);
        self.rule_repository
            .save_rule(rule)
            .await
            .map_err(|e| ValidationError::RepositoryError(e.to_string()))
    }

    /// Remove a validation rule
    pub async fn remove_validation_rule(&self, rule_id: &str) -> Result<(), ValidationError> {
        info!("Removing validation rule: {}", rule_id);
        self.rule_repository
            .delete_rule(rule_id)
            .await
            .map_err(|e| ValidationError::RepositoryError(e.to_string()))
    }
}
