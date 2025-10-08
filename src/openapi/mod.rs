//! OpenAPI Documentation for Hodei Artifacts API
//!
//! This module provides OpenAPI 3.0 specification and Swagger UI integration
//! for the Hodei Artifacts API.

use utoipa::OpenApi;

/// Main OpenAPI documentation structure
///
/// This struct aggregates all API endpoints, schemas, and metadata
/// to generate a complete OpenAPI 3.0 specification.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Hodei Artifacts API",
        version = "2.1.0",
        description = "REST API for Hodei Artifacts - A secure, policy-driven artifact management system with IAM integration\n\n## Features\n- **IAM Policy Management**: Full CRUD operations for IAM policies\n- **Policy Playground**: Test policies ad-hoc without persistence\n- **Schema Management**: Cedar schema registration and validation\n- **Authorization**: Cedar policy engine integration\n- **Policy Validation**: Validate Cedar policies against schemas\n- **Policy Evaluation**: Evaluate policies against authorization requests\n\n## Architecture\n- Clean Architecture + Vertical Slice Architecture (VSA)\n- Event-driven design\n- SurrealDB backend\n- Axum web framework\n- Cedar policy engine",
        contact(
            name = "Hodei Team",
            email = "support@hodei.io"
        ),
        license(
            name = "MIT",
            url = "https://opensource.org/licenses/MIT"
        )
    ),
    servers(
        (url = "http://localhost:3000", description = "Local development server"),
        (url = "http://localhost:8080", description = "Alternative local server"),
        (url = "https://api.hodei.io", description = "Production server")
    ),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "schemas", description = "Cedar schema management"),
        (name = "policies", description = "Policy validation and evaluation"),
        (name = "iam", description = "IAM policy management (CRUD)"),
        (name = "playground", description = "Policy playground for ad-hoc testing")
    ),
    paths(
        // Health endpoints
        crate::handlers::health::health_check,

        // Schema management endpoints
        crate::handlers::schemas::build_schema,
        crate::handlers::schemas::load_schema,
        crate::handlers::schemas::register_iam_schema,

        // Policy validation endpoints
        crate::handlers::policies::validate_policy,
        crate::handlers::policies::evaluate_policies,

        // IAM policy management endpoints
        crate::handlers::iam::create_policy,
        crate::handlers::iam::get_policy,
        crate::handlers::iam::list_policies,
        crate::handlers::iam::update_policy,
        crate::handlers::iam::delete_policy,

        // Playground endpoints
        crate::handlers::playground::playground_evaluate,
    ),
    components(
        schemas(
            // Health schemas
            crate::handlers::health::HealthResponse,

            // Schema management schemas
            crate::handlers::schemas::BuildSchemaRequest,
            crate::handlers::schemas::BuildSchemaResponse,
            crate::handlers::schemas::RegisterIamSchemaRequest,
            crate::handlers::schemas::RegisterIamSchemaResponse,

            // Policy validation schemas
            crate::handlers::policies::ValidatePolicyRequest,
            crate::handlers::policies::ValidatePolicyResponse,
            crate::handlers::policies::EvaluatePoliciesRequest,
            crate::handlers::policies::EvaluatePoliciesResponse,

            // IAM policy management schemas
            crate::handlers::iam::CreatePolicyRequest,
            crate::handlers::iam::CreatePolicyResponse,
            crate::handlers::iam::GetPolicyRequest,
            crate::handlers::iam::GetPolicyResponse,
            crate::handlers::iam::ListPoliciesQueryParams,
            crate::handlers::iam::ListPoliciesResponse,
            crate::handlers::iam::PolicySummary,
            crate::handlers::iam::PageInfo,
            crate::handlers::iam::UpdatePolicyRequest,
            crate::handlers::iam::UpdatePolicyResponse,
            crate::handlers::iam::DeletePolicyRequest,
            crate::handlers::iam::DeletePolicyResponse,

            // Playground schemas
            crate::handlers::playground::PlaygroundEvaluateRequest,
            crate::handlers::playground::PlaygroundEvaluateResponse,
            crate::handlers::playground::PlaygroundAuthorizationRequestDto,
            crate::handlers::playground::AttributeValueDto,
            crate::handlers::playground::DeterminingPolicyDto,
            crate::handlers::playground::EvaluationDiagnosticsDto,
        )
    )
)]
pub struct ApiDoc;

/// Helper function to create OpenAPI documentation
pub fn create_api_doc() -> utoipa::openapi::OpenApi {
    ApiDoc::openapi()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_generation() {
        let openapi = create_api_doc();

        // Verify basic structure
        assert_eq!(openapi.info.title, "Hodei Artifacts API");
        assert_eq!(openapi.info.version, "2.1.0");

        // Verify servers are configured
        assert!(openapi.servers.is_some());
        let servers = openapi.servers.as_ref().unwrap();
        assert!(!servers.is_empty());

        // Verify tags are present
        assert!(openapi.tags.is_some());
        let tags = openapi.tags.as_ref().unwrap();
        assert!(tags.iter().any(|t| t.name == "health"));
        assert!(tags.iter().any(|t| t.name == "iam"));
        assert!(tags.iter().any(|t| t.name == "playground"));
        assert!(tags.iter().any(|t| t.name == "schemas"));
        assert!(tags.iter().any(|t| t.name == "policies"));
    }

    #[test]
    fn test_openapi_json_serialization() {
        let openapi = create_api_doc();
        let json = serde_json::to_string(&openapi);
        assert!(json.is_ok());

        let json_str = json.unwrap();
        assert!(json_str.contains("Hodei Artifacts API"));
        assert!(json_str.contains("openapi"));
        assert!(json_str.contains("paths"));
        assert!(json_str.contains("components"));
    }
}
