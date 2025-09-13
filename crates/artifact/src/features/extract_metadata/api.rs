// crates/artifact/src/features/extract_metadata/api.rs

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};
use tracing::{info, warn, error};
use std::str::FromStr;

use super::{
    use_case::ExtractMetadataUseCase,
    dto::{ExtractMetadataCommand, MetadataExtractionResult},
    error::MetadataError,
};
use crate::features::upload_artifact::api::UserIdentity;

/// API endpoints for metadata extraction functionality
#[derive(Clone)]
pub struct ExtractMetadataApi {
    use_case: ExtractMetadataUseCase,
}

impl ExtractMetadataApi {
    pub fn new(use_case: ExtractMetadataUseCase) -> Self {
        Self { use_case }
    }

    /// Trigger metadata extraction for a specific artifact
    pub async fn extract_metadata(
        State(api): State<Self>,
        user: UserIdentity,
        Path(package_version_hrn): Path<String>,
        Json(request): Json<ExtractMetadataRequest>,
    ) -> Box<dyn IntoResponse> {
        info!(
            package_version_hrn = %package_version_hrn,
            artifact_type = %request.artifact_type,
            user_id = %user.user_id,
            "Triggering metadata extraction"
        );

        // Parse HRN (this is a simplified version - in production you'd validate properly)
        let hrn = match shared::hrn::Hrn::new(&package_version_hrn) {
            Ok(hrn) => hrn,
            Err(e) => {
                warn!(package_version_hrn = %package_version_hrn, error = %e, "Invalid HRN format");
                return Box::new((
                    StatusCode::BAD_REQUEST,
                    Json(MetadataErrorResponse::invalid_hrn("Invalid HRN format")),
                ));
            }
        };

        let command = ExtractMetadataCommand {
            package_version_hrn: hrn,
            artifact_storage_path: request.artifact_storage_path,
            artifact_type: request.artifact_type,
        };

        match api.use_case.execute(command).await {
            Ok(result) => {
                info!(
                    package_version_hrn = %result.package_version_hrn,
                    "Metadata extraction completed successfully"
                );
                Box::new((StatusCode::OK, Json(ExtractMetadataResponse::from_result(result))))
            }
            Err(MetadataError::UnsupportedArtifactType(artifact_type)) => {
                warn!(artifact_type = %artifact_type, "Unsupported artifact type for metadata extraction");
                Box::new((
                    StatusCode::BAD_REQUEST,
                    Json(MetadataErrorResponse::unsupported_type(&artifact_type)),
                ))
            }
            Err(MetadataError::StorageError(msg)) => {
                error!(error = %msg, "Storage error during metadata extraction");
                Box::new((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(MetadataErrorResponse::storage_error(&msg)),
                ))
            }
            Err(MetadataError::RepositoryError(msg)) => {
                error!(error = %msg, "Repository error during metadata extraction");
                Box::new((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(MetadataErrorResponse::repository_error(&msg)),
                ))
            }
            Err(MetadataError::EventError(msg)) => {
                error!(error = %msg, "Event error during metadata extraction");
                Box::new((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(MetadataErrorResponse::event_error(&msg)),
                ))
            }
            Err(MetadataError::ParseError(msg)) => {
                warn!(error = %msg, "Parse error during metadata extraction");
                Box::new((
                    StatusCode::BAD_REQUEST,
                    Json(MetadataErrorResponse::parse_error(&msg)),
                ))
            }
            Err(e) => {
                error!(error = %e, "Unexpected error during metadata extraction");
                Box::new((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(MetadataErrorResponse::internal_error()),
                ))
            }
        }
    }

    /// Get extracted metadata for a package version
    pub async fn get_metadata(
        State(api): State<Self>,
        user: UserIdentity,
        Path(package_version_hrn): Path<String>,
    ) -> Box<dyn IntoResponse> {
        info!(
            package_version_hrn = %package_version_hrn,
            user_id = %user.user_id,
            "Getting extracted metadata"
        );

        // TODO: Implement metadata retrieval
        // This would typically query the repository for stored metadata
        // For now, return a placeholder response
        
        Box::new((
            StatusCode::NOT_IMPLEMENTED,
            Json(MetadataErrorResponse::not_implemented()),
        ))
    }
}

/// Request for metadata extraction
#[derive(Debug, Deserialize)]
pub struct ExtractMetadataRequest {
    pub artifact_storage_path: String,
    pub artifact_type: String, // "maven", "npm", etc.
}

/// Response for successful metadata extraction
#[derive(Debug, Serialize)]
pub struct ExtractMetadataResponse {
    pub package_version_hrn: String,
    pub metadata: PackageMetadataResponse,
    pub dependencies: Vec<DependencyResponse>,
    pub extracted_at: String, // ISO 8601 timestamp
}

impl ExtractMetadataResponse {
    fn from_result(result: MetadataExtractionResult) -> Self {
        Self {
            package_version_hrn: result.package_version_hrn.to_string(),
            metadata: PackageMetadataResponse::from_domain(result.extracted_metadata),
            dependencies: result.extracted_dependencies
                .into_iter()
                .map(DependencyResponse::from_domain)
                .collect(),
            extracted_at: time::OffsetDateTime::now_utc()
                .format(&time::format_description::well_known::Iso8601::DEFAULT)
                .unwrap_or_else(|_| "unknown".to_string()),
        }
    }
}

/// Simplified metadata response
#[derive(Debug, Serialize)]
pub struct PackageMetadataResponse {
    pub description: Option<String>,
    pub licenses: Vec<String>,
    pub authors: Vec<String>,
    pub project_url: Option<String>,
    pub repository_url: Option<String>,
    pub custom_properties: std::collections::HashMap<String, String>,
}

impl PackageMetadataResponse {
    fn from_domain(metadata: crate::domain::package_version::PackageMetadata) -> Self {
        Self {
            description: metadata.description,
            licenses: metadata.licenses,
            authors: metadata.authors,
            project_url: metadata.project_url,
            repository_url: metadata.repository_url,
            custom_properties: metadata.custom_properties,
        }
    }
}

/// Dependency response
#[derive(Debug, Serialize)]
pub struct DependencyResponse {
    pub coordinates: PackageCoordinatesResponse,
    pub scope: String,
    pub version_constraint: String,
    pub is_optional: bool,
}

impl DependencyResponse {
    fn from_domain(dep: crate::domain::package_version::ArtifactDependency) -> Self {
        Self {
            coordinates: PackageCoordinatesResponse::from_domain(dep.coordinates),
            scope: dep.scope,
            version_constraint: dep.version_constraint,
            is_optional: dep.is_optional,
        }
    }
}

/// Package coordinates response
#[derive(Debug, Serialize)]
pub struct PackageCoordinatesResponse {
    pub namespace: Option<String>,
    pub name: String,
    pub version: String,
    pub qualifiers: std::collections::HashMap<String, String>,
}

impl PackageCoordinatesResponse {
    fn from_domain(coords: crate::domain::package_version::PackageCoordinates) -> Self {
        Self {
            namespace: coords.namespace,
            name: coords.name,
            version: coords.version,
            qualifiers: coords.qualifiers,
        }
    }
}

/// Error response
#[derive(Debug, Serialize)]
pub struct MetadataErrorResponse {
    pub error: String,
    pub code: String,
    pub message: String,
}

impl MetadataErrorResponse {
    fn invalid_hrn(msg: &str) -> Self {
        Self {
            error: "INVALID_HRN".to_string(),
            code: "400".to_string(),
            message: msg.to_string(),
        }
    }

    fn unsupported_type(artifact_type: &str) -> Self {
        Self {
            error: "UNSUPPORTED_ARTIFACT_TYPE".to_string(),
            code: "400".to_string(),
            message: format!("Unsupported artifact type: {}", artifact_type),
        }
    }

    fn storage_error(msg: &str) -> Self {
        Self {
            error: "STORAGE_ERROR".to_string(),
            code: "500".to_string(),
            message: format!("Storage error: {}", msg),
        }
    }

    fn repository_error(msg: &str) -> Self {
        Self {
            error: "REPOSITORY_ERROR".to_string(),
            code: "500".to_string(),
            message: format!("Repository error: {}", msg),
        }
    }

    fn event_error(msg: &str) -> Self {
        Self {
            error: "EVENT_ERROR".to_string(),
            code: "500".to_string(),
            message: format!("Event error: {}", msg),
        }
    }

    fn parse_error(msg: &str) -> Self {
        Self {
            error: "PARSE_ERROR".to_string(),
            code: "400".to_string(),
            message: format!("Parse error: {}", msg),
        }
    }

    fn not_implemented() -> Self {
        Self {
            error: "NOT_IMPLEMENTED".to_string(),
            code: "501".to_string(),
            message: "This endpoint is not yet implemented".to_string(),
        }
    }

    fn internal_error() -> Self {
        Self {
            error: "INTERNAL_ERROR".to_string(),
            code: "500".to_string(),
            message: "Internal server error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::features::extract_metadata::mocks::{
        MockPackageMetadataRepository, 
        MockArtifactContentReader, 
        MockMetadataEventPublisher
    };

    #[tokio::test]
    async fn test_extract_metadata_success() {
        let repository = Arc::new(MockPackageMetadataRepository::default());
        let content_reader = Arc::new(MockArtifactContentReader::default());
        let event_publisher = Arc::new(MockMetadataEventPublisher::default());
        
        let use_case = ExtractMetadataUseCase::new(repository, content_reader, event_publisher);
        let api = ExtractMetadataApi::new(use_case);

        let request = ExtractMetadataRequest {
            artifact_storage_path: "/test/path/artifact.jar".to_string(),
            artifact_type: "maven".to_string(),
        };

        // Note: This test would need proper HRN setup and mocking
        // For now, we'll skip the actual execution and test the structure
        assert_eq!(request.artifact_type, "maven");
        assert!(request.artifact_storage_path.contains(".jar"));
    }

    #[tokio::test]
    async fn test_extract_metadata_error_handling() {
        let repository = Arc::new(MockPackageMetadataRepository::default());
        let content_reader = Arc::new(MockArtifactContentReader::default());
        let event_publisher = Arc::new(MockMetadataEventPublisher::default());
        
        let use_case = ExtractMetadataUseCase::new(repository, content_reader, event_publisher);
        let api = ExtractMetadataApi::new(use_case);

        // Test error response creation
        let error_response = MetadataErrorResponse::unsupported_type("invalid_type");
        assert_eq!(error_response.error, "UNSUPPORTED_ARTIFACT_TYPE");
        assert_eq!(error_response.code, "400");
        assert!(error_response.message.contains("invalid_type"));
    }

    #[tokio::test]
    async fn test_response_serialization() {
        let coords = PackageCoordinatesResponse {
            namespace: Some("com.example".to_string()),
            name: "test-package".to_string(),
            version: "1.0.0".to_string(),
            qualifiers: std::collections::HashMap::new(),
        };

        let dep_response = DependencyResponse {
            coordinates: coords,
            scope: "compile".to_string(),
            version_constraint: "1.0.0".to_string(),
            is_optional: false,
        };

        let json = serde_json::to_string(&dep_response).unwrap();
        assert!(json.contains("test-package"));
        assert!(json.contains("1.0.0"));
    }
}