use axum::{extract::Path, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use shared::hrn::Hrn;
use tracing::{error, info};

use artifact::features::versioning::dto::{ValidateVersionCommand, VersionValidationResult, VersioningConfig};
use artifact::features::versioning::use_case::VersioningUseCase;

pub async fn validate_version(
    axum::Extension(use_case): axum::Extension<VersioningUseCase>,
    Json(request): Json<ValidateVersionRequest>,
) -> Result<Json<VersionValidationResult>, (StatusCode, String)> {
    info!("Validating version {} for package {}", request.version, request.package_hrn);

    let command = ValidateVersionCommand {
        package_hrn: request.package_hrn,
        version: request.version,
        repository_hrn: request.repository_hrn,
    };

    match use_case.execute(command).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => {
            error!("Version validation failed: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn get_versioning_config(
    axum::Extension(use_case): axum::Extension<VersioningUseCase>,
    Path(repository_hrn): Path<String>,
) -> Result<Json<VersioningConfig>, (StatusCode, String)> {
    info!("Getting versioning config for repository: {}", repository_hrn);

    let hrn = Hrn::new(&repository_hrn).map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid HRN format: {}", e)))?;

    match use_case.get_versioning_config(&hrn).await {
        Ok(config) => Ok(Json(config)),
        Err(e) => {
            error!("Failed to get versioning config: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn update_versioning_config(
    axum::Extension(use_case): axum::Extension<VersioningUseCase>,
    Path(repository_hrn): Path<String>,
    Json(request): Json<UpdateVersioningConfigRequest>,
) -> Result<StatusCode, (StatusCode, String)> {
    info!("Updating versioning config for repository: {}", repository_hrn);

    let hrn = Hrn::new(&repository_hrn).map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid HRN format: {}", e)))?;

    match use_case.update_versioning_config(&hrn, &request.config).await {
        Ok(()) => Ok(StatusCode::OK),
        Err(e) => {
            error!("Failed to update versioning config: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

pub async fn get_existing_versions(
    axum::Extension(use_case): axum::Extension<VersioningUseCase>,
    Path(package_hrn): Path<String>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    info!("Getting existing versions for package: {}", package_hrn);

    let hrn = Hrn::new(&package_hrn).map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid HRN format: {}", e)))?;

    match use_case.get_existing_versions(&hrn).await {
        Ok(versions) => Ok(Json(versions)),
        Err(e) => {
            error!("Failed to get existing versions: {}", e);
            Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ValidateVersionRequest {
    pub package_hrn: Hrn,
    pub version: String,
    pub repository_hrn: Hrn,
}

#[derive(Debug, Deserialize)]
pub struct UpdateVersioningConfigRequest {
    pub config: VersioningConfig,
}

#[derive(Debug, Serialize)]
pub struct ExistingVersionsResponse {
    pub versions: Vec<String>,
}
