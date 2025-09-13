use std::sync::Arc;
use axum::{extract::Path, Json, http::StatusCode};
use tracing::{info, error};
use serde::{Deserialize, Serialize};
use shared::hrn::Hrn;
use crate::domain::package_version::PackageCoordinates;
use super::{
    use_case::ValidationEngineUseCase,
    dto::{ValidateArtifactCommand, ValidationResult, ValidationRule},
    error::ValidationError,
};

/// API endpoints for validation engine functionality
#[derive(Clone)]
pub struct ValidationEngineApi {
    use_case: ValidationEngineUseCase,
}

impl ValidationEngineApi {
    pub fn new(use_case: ValidationEngineUseCase) -> Self {
        Self { use_case }
    }
    
    /// Validate an artifact
    pub async fn validate_artifact(
        &self,
        Json(request): Json<ValidateArtifactRequest>,
    ) -> Result<Json<ValidationResult>, (StatusCode, String)> {
        info!("Validating artifact: {}", request.package_version_hrn);
        
        let command = ValidateArtifactCommand {
            package_version_hrn: request.package_version_hrn,
            artifact_storage_path: request.artifact_storage_path,
            artifact_type: request.artifact_type,
            coordinates: request.coordinates,
            content_length: request.content_length,
        };
        
        match self.use_case.execute(command).await {
            Ok(result) => Ok(Json(result)),
            Err(e) => {
                error!("Validation failed: {}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
    }
    
    /// Get active validation rules for an artifact type
    pub async fn get_validation_rules(
        &self,
        Path(artifact_type): Path<String>,
    ) -> Result<Json<Vec<ValidationRule>>, (StatusCode, String)> {
        info!("Getting validation rules for artifact type: {}", artifact_type);
        
        match self.use_case.get_active_rules(&artifact_type).await {
            Ok(rules) => Ok(Json(rules)),
            Err(e) => {
                error!("Failed to get validation rules: {}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
    }
    
    /// Add a new validation rule
    pub async fn add_validation_rule(
        &self,
        Json(request): Json<AddValidationRuleRequest>,
    ) -> Result<StatusCode, (StatusCode, String)> {
        info!("Adding validation rule: {}", request.name);
        
        let rule = ValidationRule {
            id: request.id,
            name: request.name,
            description: request.description,
            enabled: request.enabled,
            priority: request.priority,
            artifact_types: request.artifact_types,
            rule_type: request.rule_type,
        };
        
        match self.use_case.add_validation_rule(&rule).await {
            Ok(()) => Ok(StatusCode::CREATED),
            Err(e) => {
                error!("Failed to add validation rule: {}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
    }
    
    /// Remove a validation rule
    pub async fn remove_validation_rule(
        &self,
        Path(rule_id): Path<String>,
    ) -> Result<StatusCode, (StatusCode, String)> {
        info!("Removing validation rule: {}", rule_id);
        
        match self.use_case.remove_validation_rule(&rule_id).await {
            Ok(()) => Ok(StatusCode::NO_CONTENT),
            Err(e) => {
                error!("Failed to remove validation rule: {}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
    }
}

/// Request to validate an artifact
#[derive(Debug, Deserialize)]
pub struct ValidateArtifactRequest {
    pub package_version_hrn: Hrn,
    pub artifact_storage_path: String,
    pub artifact_type: String,
    pub coordinates: PackageCoordinates,
    pub content_length: u64,
}

/// Request to add a validation rule
#[derive(Debug, Deserialize)]
pub struct AddValidationRuleRequest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub priority: u32,
    pub artifact_types: Vec<String>,
    pub rule_type: super::dto::ValidationRuleType,
}

/// Response for validation rules endpoint
#[derive(Debug, Serialize)]
pub struct ValidationRulesResponse {
    pub rules: Vec<ValidationRule>,
}