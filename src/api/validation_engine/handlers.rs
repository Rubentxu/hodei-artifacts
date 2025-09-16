use axum::{extract::Path, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use artifact::domain::package_version::PackageCoordinates;
use artifact::features::validation_engine::dto::{ValidationRule, ValidationRuleType};
use artifact::features::validation_engine::use_case::ValidationEngineUseCase;
use artifact::features::validation_engine::{dto::ValidateArtifactCommand, dto::ValidationResult};

pub async fn validate_artifact(
    axum::Extension(use_case): axum::Extension<ValidationEngineUseCase>,
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

    match use_case.execute(command).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => {
            error!("Validation failed: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn get_validation_rules(
    axum::Extension(use_case): axum::Extension<ValidationEngineUseCase>,
    Path(artifact_type): Path<String>,
) -> Result<Json<Vec<ValidationRule>>, (StatusCode, String)> {
    info!("Getting validation rules for artifact type: {}", artifact_type);

    match use_case.get_active_rules(&artifact_type).await {
        Ok(rules) => Ok(Json(rules)),
        Err(e) => {
            error!("Failed to get validation rules: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn add_validation_rule(
    axum::Extension(use_case): axum::Extension<ValidationEngineUseCase>,
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

    match use_case.add_validation_rule(&rule).await {
        Ok(()) => Ok(StatusCode::CREATED),
        Err(e) => {
            error!("Failed to add validation rule: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn remove_validation_rule(
    axum::Extension(use_case): axum::Extension<ValidationEngineUseCase>,
    Path(rule_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    info!("Removing validation rule: {}", rule_id);

    match use_case.remove_validation_rule(&rule_id).await {
        Ok(()) => Ok(StatusCode::NO_CONTENT),
        Err(e) => {
            error!("Failed to remove validation rule: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ValidateArtifactRequest {
    pub package_version_hrn: shared::hrn::Hrn,
    pub artifact_storage_path: String,
    pub artifact_type: String,
    pub coordinates: PackageCoordinates,
    pub content_length: u64,
}

#[derive(Debug, Deserialize)]
pub struct AddValidationRuleRequest {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub priority: u32,
    pub artifact_types: Vec<String>,
    pub rule_type: ValidationRuleType,
}

#[derive(Debug, Serialize)]
pub struct ValidationRulesResponse {
    pub rules: Vec<ValidationRule>,
}
