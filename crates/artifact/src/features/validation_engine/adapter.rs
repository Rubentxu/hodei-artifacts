use super::{
    dto::{RuleValidationOutcome, ValidationContext, ValidationRule},
    error::ValidationError,
    ports::{
        ArtifactContentReader, ValidationEventPublisher, ValidationRuleExecutor,
        ValidationRuleRepository,
    },
};
use crate::features::upload_artifact::ports::ArtifactStorage;
use async_trait::async_trait;
use bytes::Bytes;
use serde_json;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use tracing::{debug, warn};
use zip::ZipArchive;

/// Adapter for reading artifact content from storage
pub struct StorageArtifactContentReader {
    storage: Arc<dyn ArtifactStorage>,
}

impl StorageArtifactContentReader {
    pub fn new(storage: Arc<dyn ArtifactStorage>) -> Self {
        Self { storage }
    }
}

#[async_trait]
impl ArtifactContentReader for StorageArtifactContentReader {
    async fn read_artifact_content(&self, storage_path: &str) -> Result<Bytes, ValidationError> {
        debug!(
            "Reading artifact content from storage path: {}",
            storage_path
        );
        // In a real implementation, we would download the content from storage
        // For now, we'll return an error as this requires integration with storage
        Err(ValidationError::StorageError("Not implemented: Reading artifact content from storage path requires downloading the file".to_string()))
    }
}

/// Adapter for managing validation rules in repository
pub struct RepositoryValidationRuleManager {
    // In a real implementation, this would hold a reference to the repository
}

impl RepositoryValidationRuleManager {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl ValidationRuleRepository for RepositoryValidationRuleManager {
    async fn get_active_rules_for_artifact_type(
        &self,
        artifact_type: &str,
    ) -> Result<Vec<ValidationRule>, ValidationError> {
        debug!(
            "Getting active validation rules for artifact type: {}",
            artifact_type
        );
        // In a real implementation, this would query the database
        // For now, return some default rules
        let mut rules = Vec::new();

        if artifact_type == "maven" || artifact_type == "jar" {
            rules.push(ValidationRule {
                id: "jar-signature-required".to_string(),
                name: "JAR Signature Required".to_string(),
                description: "JAR files must be digitally signed".to_string(),
                enabled: true,
                priority: 1,
                artifact_types: vec!["maven".to_string(), "jar".to_string()],
                rule_type: super::dto::ValidationRuleType::JarSignatureRequired,
            });
        }

        if artifact_type == "npm" || artifact_type == "package.json" {
            rules.push(ValidationRule {
                id: "npm-license-required".to_string(),
                name: "NPM License Required".to_string(),
                description: "package.json must have a license field".to_string(),
                enabled: true,
                priority: 1,
                artifact_types: vec!["npm".to_string(), "package.json".to_string()],
                rule_type: super::dto::ValidationRuleType::NpmLicenseRequired,
            });
        }

        // Size limit rule for all artifact types
        rules.push(ValidationRule {
            id: "size-limit-100mb".to_string(),
            name: "Size Limit 100MB".to_string(),
            description: "Artifact size must not exceed 100MB".to_string(),
            enabled: true,
            priority: 2,
            artifact_types: vec!["*".to_string()],
            rule_type: super::dto::ValidationRuleType::SizeLimit {
                max_size_bytes: 100 * 1024 * 1024,
            },
        });

        Ok(rules)
    }

    async fn get_rule_by_id(
        &self,
        rule_id: &str,
    ) -> Result<Option<ValidationRule>, ValidationError> {
        debug!("Getting validation rule by ID: {}", rule_id);
        // In a real implementation, this would query the database
        Ok(None)
    }

    async fn save_rule(&self, rule: &ValidationRule) -> Result<(), ValidationError> {
        debug!("Saving validation rule: {}", rule.name);
        // In a real implementation, this would save to the database
        Ok(())
    }

    async fn delete_rule(&self, rule_id: &str) -> Result<(), ValidationError> {
        debug!("Deleting validation rule: {}", rule_id);
        // In a real implementation, this would delete from the database
        Ok(())
    }
}

/// Adapter for publishing validation events
pub struct EventBusValidationPublisher {
    // In a real implementation, this would hold a reference to the event publisher
}

impl EventBusValidationPublisher {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl ValidationEventPublisher for EventBusValidationPublisher {
    async fn publish_validation_failed(
        &self,
        event: crate::domain::events::ArtifactEvent,
    ) -> Result<(), ValidationError> {
        debug!("Publishing validation failed event");
        // In a real implementation, this would publish the event to the event bus
        Ok(())
    }
}

/// Adapter for executing validation rules
pub struct DefaultValidationRuleExecutor;

impl DefaultValidationRuleExecutor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ValidationRuleExecutor for DefaultValidationRuleExecutor {
    async fn execute_rule(
        &self,
        rule: &ValidationRule,
        context: &ValidationContext,
    ) -> Result<RuleValidationOutcome, ValidationError> {
        debug!("Executing validation rule: {}", rule.name);

        match &rule.rule_type {
            super::dto::ValidationRuleType::JarSignatureRequired => {
                self.validate_jar_signature(context).await
            }
            super::dto::ValidationRuleType::NpmLicenseRequired => {
                self.validate_npm_license(context).await
            }
            super::dto::ValidationRuleType::SizeLimit { max_size_bytes } => {
                self.validate_size_limit(context, *max_size_bytes).await
            }
            super::dto::ValidationRuleType::Custom {
                script_path,
                parameters,
            } => {
                self.validate_custom_rule(context, script_path, parameters)
                    .await
            }
        }
    }
}

impl DefaultValidationRuleExecutor {
    /// Validate JAR signature
    async fn validate_jar_signature(
        &self,
        context: &ValidationContext,
    ) -> Result<RuleValidationOutcome, ValidationError> {
        if !context.artifact_type.contains("jar") && !context.artifact_type.contains("maven") {
            // Rule doesn't apply to this artifact type
            return Ok(RuleValidationOutcome {
                rule_id: "jar-signature-required".to_string(),
                passed: true,
                errors: Vec::new(),
                warnings: Vec::new(),
            });
        }

        // For JAR files, check if they have signature files
        let cursor = Cursor::new(context.artifact_content.as_ref());
        let mut archive = match ZipArchive::new(cursor) {
            Ok(archive) => archive,
            Err(e) => {
                return Ok(RuleValidationOutcome {
                    rule_id: "jar-signature-required".to_string(),
                    passed: false,
                    errors: vec![format!("Invalid JAR file: {}", e)],
                    warnings: Vec::new(),
                });
            }
        };

        let mut has_signature = false;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let name = file.name();

            if name.starts_with("META-INF/")
                && (name.ends_with(".SF") || name.ends_with(".RSA") || name.ends_with(".DSA"))
            {
                has_signature = true;
                break;
            }
        }

        Ok(RuleValidationOutcome {
            rule_id: "jar-signature-required".to_string(),
            passed: has_signature,
            errors: if !has_signature {
                vec!["JAR file is not digitally signed".to_string()]
            } else {
                Vec::new()
            },
            warnings: Vec::new(),
        })
    }

    /// Validate NPM license requirement
    async fn validate_npm_license(
        &self,
        context: &ValidationContext,
    ) -> Result<RuleValidationOutcome, ValidationError> {
        if !context.artifact_type.contains("npm") && !context.artifact_type.contains("package.json")
        {
            // Rule doesn't apply to this artifact type
            return Ok(RuleValidationOutcome {
                rule_id: "npm-license-required".to_string(),
                passed: true,
                errors: Vec::new(),
                warnings: Vec::new(),
            });
        }

        let content_str = String::from_utf8_lossy(&context.artifact_content);

        // Try to parse as JSON
        let package: serde_json::Value = match serde_json::from_str(&content_str) {
            Ok(package) => package,
            Err(e) => {
                return Ok(RuleValidationOutcome {
                    rule_id: "npm-license-required".to_string(),
                    passed: false,
                    errors: vec![format!("Invalid package.json: {}", e)],
                    warnings: Vec::new(),
                });
            }
        };

        let has_license = package.get("license").is_some() || package.get("licenses").is_some();

        Ok(RuleValidationOutcome {
            rule_id: "npm-license-required".to_string(),
            passed: has_license,
            errors: if !has_license {
                vec!["package.json must have a license field".to_string()]
            } else {
                Vec::new()
            },
            warnings: Vec::new(),
        })
    }

    /// Validate size limit
    async fn validate_size_limit(
        &self,
        context: &ValidationContext,
        max_size_bytes: u64,
    ) -> Result<RuleValidationOutcome, ValidationError> {
        let is_valid = context.content_length <= max_size_bytes;

        Ok(RuleValidationOutcome {
            rule_id: "size-limit".to_string(),
            passed: is_valid,
            errors: if !is_valid {
                vec![format!(
                    "Artifact size {} exceeds maximum allowed size {}",
                    context.content_length, max_size_bytes
                )]
            } else {
                Vec::new()
            },
            warnings: Vec::new(),
        })
    }

    /// Execute custom validation rule
    async fn validate_custom_rule(
        &self,
        _context: &ValidationContext,
        script_path: &str,
        _parameters: &HashMap<String, String>,
    ) -> Result<RuleValidationOutcome, ValidationError> {
        warn!(
            "Custom validation rules not yet implemented: {}",
            script_path
        );

        Ok(RuleValidationOutcome {
            rule_id: "custom-rule".to_string(),
            passed: false,
            errors: vec!["Custom validation rules not yet implemented".to_string()],
            warnings: Vec::new(),
        })
    }
}
