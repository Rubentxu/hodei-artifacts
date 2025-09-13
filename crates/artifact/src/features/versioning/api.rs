use std::sync::Arc;
use axum::{extract::Path, Json, http::StatusCode};
use tracing::{info, error};
use serde::{Deserialize, Serialize};
use shared::hrn::Hrn;
use super::{
    use_case::VersioningUseCase,
    dto::{ValidateVersionCommand, VersionValidationResult, VersioningConfig},
    error::VersioningError,
};

/// API endpoints for versioning functionality
#[derive(Clone)]
pub struct VersioningApi {
    use_case: VersioningUseCase,
}

impl VersioningApi {
    pub fn new(use_case: VersioningUseCase) -> Self {
        Self { use_case }
    }
    
    /// Validate a version
    pub async fn validate_version(
        &self,
        Json(request): Json<ValidateVersionRequest>,
    ) -> Result<Json<VersionValidationResult>, (StatusCode, String)> {
        info!("Validating version {} for package {}", request.version, request.package_hrn);
        
        let command = ValidateVersionCommand {
            package_hrn: request.package_hrn,
            version: request.version,
            repository_hrn: request.repository_hrn,
        };
        
        match self.use_case.execute(command).await {
            Ok(result) => Ok(Json(result)),
            Err(e) => {
                error!("Version validation failed: {}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
    }
    
    /// Get versioning configuration for a repository
    pub async fn get_versioning_config(
        &self,
        Path(repository_hrn): Path<String>,
    ) -> Result<Json<VersioningConfig>, (StatusCode, String)> {
        info!("Getting versioning config for repository: {}", repository_hrn);
        
        let hrn = match Hrn::new(&repository_hrn) {
            Ok(hrn) => hrn,
            Err(e) => {
                error!("Invalid HRN format: {}", e);
                return Err((StatusCode::BAD_REQUEST, format!("Invalid HRN format: {}", e)));
            }
        };
        
        match self.use_case.get_versioning_config(&hrn).await {
            Ok(config) => Ok(Json(config)),
            Err(e) => {
                error!("Failed to get versioning config: {}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
    }
    
    /// Update versioning configuration for a repository
    pub async fn update_versioning_config(
        &self,
        Path(repository_hrn): Path<String>,
        Json(request): Json<UpdateVersioningConfigRequest>,
    ) -> Result<StatusCode, (StatusCode, String)> {
        info!("Updating versioning config for repository: {}", repository_hrn);
        
        let hrn = match Hrn::new(&repository_hrn) {
            Ok(hrn) => hrn,
            Err(e) => {
                error!("Invalid HRN format: {}", e);
                return Err((StatusCode::BAD_REQUEST, format!("Invalid HRN format: {}", e)));
            }
        };
        
        match self.use_case.update_versioning_config(&hrn, &request.config).await {
            Ok(()) => Ok(StatusCode::OK),
            Err(e) => {
                error!("Failed to update versioning config: {}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
    }
    
    /// Get existing versions for a package
    pub async fn get_existing_versions(
        &self,
        Path(package_hrn): Path<String>,
    ) -> Result<Json<Vec<String>>, (StatusCode, String)> {
        info!("Getting existing versions for package: {}", package_hrn);
        
        let hrn = match Hrn::new(&package_hrn) {
            Ok(hrn) => hrn,
            Err(e) => {
                error!("Invalid HRN format: {}", e);
                return Err((StatusCode::BAD_REQUEST, format!("Invalid HRN format: {}", e)));
            }
        };
        
        match self.use_case.get_existing_versions(&hrn).await {
            Ok(versions) => Ok(Json(versions)),
            Err(e) => {
                error!("Failed to get existing versions: {}", e);
                Err((StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
            }
        }
    }
}

/// Request to validate a version
#[derive(Debug, Deserialize)]
pub struct ValidateVersionRequest {
    pub package_hrn: Hrn,
    pub version: String,
    pub repository_hrn: Hrn,
}

/// Request to update versioning configuration
#[derive(Debug, Deserialize)]
pub struct UpdateVersioningConfigRequest {
    pub config: VersioningConfig,
}

/// Response for existing versions endpoint
#[derive(Debug, Serialize)]
pub struct ExistingVersionsResponse {
    pub versions: Vec<String>,
}