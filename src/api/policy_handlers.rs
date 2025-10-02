use crate::{
    app_state::AppState,
    error::{AppError, Result},
};
use axum::{
    extract::{Path, Query, State},
    response::Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};

/// Request body for creating a new policy
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreatePolicyRequest {
    /// Name of the policy
    #[schema(example = "allow-read-access")]
    pub name: String,
    /// Optional description of the policy
    #[schema(example = "Allows read access to resources")]
    pub description: Option<String>,
    /// Cedar policy content
    #[schema(example = "permit(principal, action == Action::\"read\", resource);")]
    pub policy_content: String,
    /// Whether the policy is enabled
    #[schema(example = true)]
    pub enabled: Option<bool>,
}

/// Analyze policies against a set of safety rules
#[utoipa::path(
    post,
    path = "/api/v1/policies/analysis",
    request_body = AnalyzePoliciesRequestApi,
    responses(
        (status = 200, description = "Analysis result", body = AnalyzePoliciesResponseApi),
        (status = 400, description = "Invalid request", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn analyze_policies(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AnalyzePoliciesRequestApi>,
) -> Result<Json<AnalyzePoliciesResponseApi>> {
    let domain_rules: Vec<policies::features::policy_analysis::dto::AnalysisRule> = request
        .rules
        .into_iter()
        .map(|r| policies::features::policy_analysis::dto::AnalysisRule { id: r.id, kind: r.kind, params: r.params })
        .collect();
    let domain_req = policies::features::policy_analysis::dto::AnalyzePoliciesRequest {
        policies: request.policies,
        schema: request.schema,
        rules: domain_rules,
    };
    let result = state
        .analyze_policies_uc
        .execute(&domain_req)
        .await
        .map_err(AppError::BadRequest)?;

    let response = AnalyzePoliciesResponseApi {
        passed: result.passed,
        violations: result
            .violations
            .into_iter()
            .map(|v| RuleViolationApi { rule_id: v.rule_id, message: v.message })
            .collect(),
    };
    Ok(Json(response))
}

/// Batch evaluate playground scenarios with limits and timeout
#[utoipa::path(
    post,
    path = "/api/v1/policies/playground/batch",
    request_body = BatchPlaygroundRequestApi,
    responses(
        (status = 200, description = "Batch evaluation result", body = BatchPlaygroundResponseApi),
        (status = 400, description = "Invalid request", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn batch_playground(
    State(state): State<Arc<AppState>>,
    Json(request): Json<BatchPlaygroundRequestApi>,
) -> Result<Json<BatchPlaygroundResponseApi>> {
    state.metrics.record_request();
    let domain_entities: Vec<policies::features::policy_playground::dto::EntityDefinition> = request
        .entities
        .into_iter()
        .map(|e| policies::features::policy_playground::dto::EntityDefinition { uid: e.uid, attributes: e.attributes, parents: e.parents })
        .collect();
    let domain_scenarios: Vec<policies::features::policy_playground::dto::AuthorizationScenario> = request
        .scenarios
        .into_iter()
        .map(|s| policies::features::policy_playground::dto::AuthorizationScenario { name: s.name, principal: s.principal, action: s.action, resource: s.resource, context: s.context })
        .collect();

    let domain_req = policies::features::batch_eval::dto::BatchPlaygroundRequest {
        policies: request.policies,
        schema: request.schema,
        entities: domain_entities,
        scenarios: domain_scenarios,
        limit_scenarios: request.limit_scenarios,
        timeout_ms: request.timeout_ms,
    };
    let result = state
        .batch_eval_uc
        .execute(&domain_req)
        .await
        .map_err(AppError::BadRequest)?;

    let response = BatchPlaygroundResponseApi {
        results_count: result.results_count,
        statistics: EvaluationStatisticsApi {
            total_scenarios: result.statistics.total_scenarios,
            allow_count: result.statistics.allow_count,
            deny_count: result.statistics.deny_count,
            total_evaluation_time_us: result.statistics.total_evaluation_time_us,
            average_evaluation_time_us: result.statistics.average_evaluation_time_us,
        },
    };

    // Metrics: count authorizations (approximate using aggregate stats)
    for _ in 0..response.statistics.allow_count { state.metrics.record_authorization(true); }
    for _ in 0..response.statistics.deny_count { state.metrics.record_authorization(false); }

    Ok(Json(response))
}

// ============ Playground DTOs ============

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct PlaygroundRequestApi {
    /// Cedar policies to evaluate
    #[schema(example = json!( ["permit(principal, action, resource) when { context.mfa == true };"] ))]
    pub policies: Vec<String>,
    /// Optional Cedar schema string
    #[schema(example = "entity User {} action Action {} entity Resource {}")]
    pub schema: Option<String>,
    /// Entities available for evaluation
    #[serde(default)]
    #[schema(example = json!( [
        {"uid":"User::\"alice\"","attributes":{},"parents":["Group::\"admins\""]}
    ] ))]
    pub entities: Vec<EntityDefinitionApi>,
    /// Authorization scenarios to test
    #[schema(example = json!( [
        {"name":"alice","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context":{"mfa":true}}
    ] ))]
    pub authorization_requests: Vec<PlaygroundScenarioApi>,
    /// Options (optional)
    pub options: Option<PlaygroundOptionsApi>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct PlaygroundScenarioApi {
    /// Scenario name
    #[schema(example = "alice")]
    pub name: String,
    /// Principal EUID
    #[schema(example = "User::\"alice\"")]
    pub principal: String,
    /// Action EUID
    #[schema(example = "Action::\"view\"")]
    pub action: String,
    /// Resource EUID
    #[schema(example = "Resource::\"doc1\"")]
    pub resource: String,
    /// Optional JSON context
    #[schema(example = json!({"mfa": true}))]
    pub context: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct PlaygroundOptionsApi {
    /// Include diagnostics info in results
    #[schema(example = true)]
    pub include_diagnostics: bool,
    /// Include per-policy traces (determining/evaluated policies)
    #[serde(default)]
    #[schema(example = false)]
    pub include_policy_traces: bool,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlaygroundResponseApi {
    #[schema(example = json!({"is_valid": true, "errors": [], "warnings": [], "policies_count": 1}))]
    pub policy_validation: PolicyValidationApi,
    #[schema(example = json!({"is_valid": true, "errors": [], "entity_types_count": 0, "actions_count": 0}))]
    pub schema_validation: SchemaValidationApi,
    pub authorization_results: Vec<PlaygroundAuthResultApi>,
    #[schema(example = json!({"total_scenarios": 1, "allow_count": 1, "deny_count": 0, "total_evaluation_time_us": 120, "average_evaluation_time_us": 120}))]
    pub statistics: EvaluationStatisticsApi,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PlaygroundAuthResultApi {
    #[schema(example = "alice")]
    pub scenario_name: String,
    #[schema(example = "Allow")]
    pub decision: String,
    #[schema(example = json!(["matched when clause"]))]
    pub reasons: Vec<String>,
    /// Optional determining policies (only when requested)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schema(example = json!(["trace_policy_0"]))]
    pub determining_policies: Option<Vec<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ValidationErrorApi {
    pub message: String,
    pub policy_id: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ValidationWarningApi {
    pub message: String,
    pub severity: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyValidationApi {
    pub is_valid: bool,
    pub errors: Vec<ValidationErrorApi>,
    pub warnings: Vec<ValidationWarningApi>,
    pub policies_count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SchemaValidationApi {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub entity_types_count: usize,
    pub actions_count: usize,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EvaluationStatisticsApi {
    pub total_scenarios: usize,
    pub allow_count: usize,
    pub deny_count: usize,
    pub total_evaluation_time_us: u64,
    pub average_evaluation_time_us: u64,
}

// ============ Analysis DTOs ============

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct AnalyzePoliciesRequestApi {
    #[schema(example = json!( ["permit(principal, action, resource) when { context.mfa == true };"] ))]
    pub policies: Vec<String>,
    #[schema(example = json!(null))]
    pub schema: Option<String>,
    #[serde(default)]
    #[schema(example = json!( [ {"id":"r1","kind":"no_permit_without_mfa","params":{}} ] ))]
    pub rules: Vec<AnalysisRuleApi>,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct AnalysisRuleApi {
    pub id: String,
    /// Example: "no_permit_without_mfa"
    pub kind: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct AnalyzePoliciesResponseApi {
    #[schema(example = true)]
    pub passed: bool,
    #[serde(default)]
    #[schema(example = json!([]))]
    pub violations: Vec<RuleViolationApi>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct RuleViolationApi {
    pub rule_id: String,
    pub message: String,
}

// ============ Batch DTOs ============

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct BatchPlaygroundRequestApi {
    #[schema(example = json!( [
        "permit(principal, action, resource) when { context.mfa == true };",
        "forbid(principal == User::\"bob\", action, resource);"
    ] ))]
    pub policies: Vec<String>,
    #[schema(example = json!(null))]
    pub schema: Option<String>,
    #[serde(default)]
    #[schema(example = json!([]))]
    pub entities: Vec<EntityDefinitionApi>,
    #[schema(example = json!( [
        {"name":"alice","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}},
        {"name":"bob","principal":"User::\"bob\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}}
    ] ))]
    pub scenarios: Vec<PlaygroundScenarioApi>,
    #[serde(default)]
    #[schema(example = 100)]
    pub limit_scenarios: Option<usize>,
    #[serde(default)]
    #[schema(example = 2000)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct BatchPlaygroundResponseApi {
    #[schema(example = 2)]
    pub results_count: usize,
    #[schema(example = json!({"total_scenarios": 2, "allow_count": 1, "deny_count": 1, "total_evaluation_time_us": 210, "average_evaluation_time_us": 105}))]
    pub statistics: EvaluationStatisticsApi,
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct EntityDefinitionApi {
    /// Entity UID string (e.g. User::"alice")
    pub uid: String,
    /// Entity attributes as JSON
    pub attributes: std::collections::HashMap<String, serde_json::Value>,
    /// Parent entity UIDs
    pub parents: Vec<String>,
}

/// Request body for updating an existing policy
#[derive(Debug, Deserialize, ToSchema)]
pub struct UpdatePolicyRequest {
    /// Cedar policy content to replace the existing policy
    #[schema(example = "permit(principal, action, resource);")]
    pub policy_content: String,
}

/// Request body for validating a policy without persisting it
#[derive(Debug, Deserialize, ToSchema)]
pub struct ValidatePolicyRequest {
    /// Cedar policy content to validate
    #[schema(example = "permit(principal, action, resource);")]
    pub policy_content: String,
}

/// Response containing policy details
#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyResponse {
    /// Unique identifier of the policy
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    pub id: String,
    /// Name of the policy
    #[schema(example = "allow-read-access")]
    pub name: String,
    /// Optional description
    #[schema(example = "Allows read access to resources")]
    pub description: Option<String>,
    /// Cedar policy content
    #[schema(example = "permit(principal, action == Action::\"read\", resource);")]
    pub policy_content: String,
    /// Whether the policy is enabled
    pub enabled: bool,
    /// ISO 8601 timestamp of creation
    #[schema(example = "2024-01-01T12:00:00Z")]
    pub created_at: String,
    /// ISO 8601 timestamp of last update
    #[schema(example = "2024-01-01T12:00:00Z")]
    pub updated_at: String,
}

/// Response containing a list of policies
#[derive(Debug, Serialize, ToSchema)]
pub struct PolicyListResponse {
    /// List of policies
    pub policies: Vec<PolicyResponse>,
    /// Total number of policies returned (after pagination)
    pub total: usize,
    /// Offset used for pagination
    pub offset: usize,
    /// Limit used for pagination
    pub limit: Option<usize>,
}

/// Query parameters for listing policies
#[derive(Debug, Deserialize, IntoParams, ToSchema)]
pub struct ListPoliciesParams {
    /// Number of items to skip (pagination)
    #[param(example = 0)]
    #[schema(example = 0)]
    pub offset: Option<usize>,
    /// Maximum number of items to return (pagination, max 1000)
    #[param(example = 100)]
    #[schema(example = 100)]
    pub limit: Option<usize>,
    /// Filter policies by ID (partial match)
    #[param(example = "policy")]
    #[schema(example = "policy")]
    pub filter_id: Option<String>,
}

/// Error response
#[derive(Debug, Serialize, ToSchema)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,
    /// Optional error details
    pub details: Option<String>,
}

/// Create a new policy
///
/// Creates a new Cedar policy with the provided content.
#[utoipa::path(
    post,
    path = "/api/v1/policies",
    request_body = CreatePolicyRequest,
    responses(
        (status = 200, description = "Policy created successfully", body = PolicyResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn create_policy(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreatePolicyRequest>,
) -> Result<Json<PolicyResponse>> {
    tracing::info!(
        policy_name = %request.name,
        "Creating new policy"
    );

    // Validate request
    if request.name.is_empty() {
        return Err(AppError::BadRequest(
            "Policy name cannot be empty".to_string(),
        ));
    }

    if request.policy_content.is_empty() {
        return Err(AppError::BadRequest(
            "Policy content cannot be empty".to_string(),
        ));
    }

    // Build command and validate via policies DTO
    let cmd = policies::features::create_policy::dto::CreatePolicyCommand::new(
        request.policy_content.clone(),
    );
    if let Err(e) = cmd.validate() {
        return Err(AppError::Validation(e.to_string()));
    }

    // Execute use case from AppState
    state
        .create_policy_uc
        .execute(&cmd)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    // Record metrics
    state.metrics.record_policy_operation();

    // Create response (ID generated here; persistence layer stores the policy text)
    let policy_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    let policy = PolicyResponse {
        id: policy_id.clone(),
        name: request.name.clone(),
        description: request.description,
        policy_content: request.policy_content,
        enabled: request.enabled.unwrap_or(true),
        created_at: now.clone(),
        updated_at: now,
    };

    tracing::info!(
        policy_id = %policy_id,
        policy_name = %request.name,
        "Policy created successfully"
    );

    Ok(Json(policy))
}

/// List all policies
///
/// Retrieves a list of all policies in the system with optional pagination and filtering.
#[utoipa::path(
    get,
    path = "/api/v1/policies",
    params(ListPoliciesParams),
    responses(
        (status = 200, description = "List of policies retrieved successfully", body = PolicyListResponse),
        (status = 400, description = "Invalid query parameters", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn list_policies(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListPoliciesParams>,
) -> Result<Json<PolicyListResponse>> {
    tracing::debug!(
        offset = ?params.offset,
        limit = ?params.limit,
        filter_id = ?params.filter_id,
        "Listing policies with parameters"
    );

    // Build query from params
    let mut query = policies::features::list_policies::dto::ListPoliciesQuery::new();
    query.offset = params.offset;
    query.limit = params.limit;
    query.filter_id = params.filter_id;

    // Execute use case from AppState
    let cedar_policies = state
        .list_policies_uc
        .execute(&query)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    // Record metrics
    state.metrics.record_policy_operation();

    // Convert Vec<Policy> to Vec<PolicyResponse>
    let now = chrono::Utc::now().to_rfc3339();
    let policy_responses: Vec<PolicyResponse> = cedar_policies
        .iter()
        .map(|p| {
            PolicyResponse {
                id: p.id().to_string(),
                name: p.id().to_string(), // Cedar policies use ID as name
                description: None,
                policy_content: p.to_string(),
                enabled: true,
                created_at: now.clone(),
                updated_at: now.clone(),
            }
        })
        .collect();

    let response = PolicyListResponse {
        total: policy_responses.len(),
        policies: policy_responses,
        offset: query.offset.unwrap_or(0),
        limit: query.limit,
    };

    tracing::info!(
        total_policies = response.total,
        offset = response.offset,
        limit = ?response.limit,
        "Policies listed successfully"
    );

    Ok(Json(response))
}

/// Policy playground: evaluate ad-hoc policies against scenarios without persisting
#[utoipa::path(
    post,
    path = "/api/v1/policies/playground",
    request_body = PlaygroundRequestApi,
    responses(
        (status = 200, description = "Playground evaluation results", body = PlaygroundResponseApi),
        (status = 400, description = "Invalid request", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn policy_playground(
    State(state): State<Arc<AppState>>,
    Json(request): Json<PlaygroundRequestApi>,
) -> Result<Json<PlaygroundResponseApi>> {
    state.metrics.record_request();
    // Map API DTO to domain DTO
    let options = request
        .options
        .unwrap_or(PlaygroundOptionsApi { include_diagnostics: true, include_policy_traces: false });
    let domain_options = policies::features::policy_playground::dto::PlaygroundOptions {
        include_diagnostics: options.include_diagnostics,
    };
    let domain_entities: Vec<policies::features::policy_playground::dto::EntityDefinition> = request
        .entities
        .into_iter()
        .map(|e| policies::features::policy_playground::dto::EntityDefinition {
            uid: e.uid,
            attributes: e.attributes,
            parents: e.parents,
        })
        .collect();
    let domain_scenarios: Vec<policies::features::policy_playground::dto::AuthorizationScenario> = request
        .authorization_requests
        .into_iter()
        .map(|s| policies::features::policy_playground::dto::AuthorizationScenario {
            name: s.name,
            principal: s.principal,
            action: s.action,
            resource: s.resource,
            context: s.context,
        })
        .collect();
    let domain_req = policies::features::policy_playground::dto::PlaygroundRequest {
        policies: request.policies,
        schema: request.schema,
        entities: domain_entities,
        authorization_requests: domain_scenarios,
        options: Some(domain_options),
    };

    // Dispatch: with or without policy traces
    if options.include_policy_traces {
        let traced_opts = policies::features::policy_playground_traces::dto::TracedPlaygroundOptions {
            include_policy_traces: true,
        };
        let traced_uc = policies::features::policy_playground_traces::use_case::TracedPlaygroundUseCase::new();
        let traced = traced_uc
            .execute(&traced_opts, &domain_req, &state.policy_playground_uc)
            .await
            .map_err(AppError::BadRequest)?;

        let policy_validation = PolicyValidationApi {
            is_valid: traced.policy_validation.is_valid,
            errors: traced
                .policy_validation
                .errors
                .into_iter()
                .map(|e| ValidationErrorApi { message: e.message, policy_id: e.policy_id, line: e.line, column: e.column })
                .collect(),
            warnings: traced
                .policy_validation
                .warnings
                .into_iter()
                .map(|w| ValidationWarningApi {
                    message: w.message,
                    severity: match w.severity {
                        policies::features::policy_playground::dto::WarningSeverity::Low => "Low".into(),
                        policies::features::policy_playground::dto::WarningSeverity::Medium => "Medium".into(),
                        policies::features::policy_playground::dto::WarningSeverity::High => "High".into(),
                    },
                })
                .collect(),
            policies_count:  traced.policy_validation.policies_count,
        };

        let schema_validation = SchemaValidationApi {
            is_valid: traced.schema_validation.is_valid,
            errors: traced.schema_validation.errors,
            entity_types_count: traced.schema_validation.entity_types_count,
            actions_count: traced.schema_validation.actions_count,
        };

        let statistics = EvaluationStatisticsApi {
            total_scenarios: traced.statistics.total_scenarios,
            allow_count: traced.statistics.allow_count,
            deny_count: traced.statistics.deny_count,
            total_evaluation_time_us: traced.statistics.total_evaluation_time_us,
            average_evaluation_time_us: traced.statistics.average_evaluation_time_us,
        };

        let response = PlaygroundResponseApi {
            policy_validation,
            schema_validation,
            authorization_results: traced
                .authorization_results
                .into_iter()
                .map(|r| PlaygroundAuthResultApi {
                    scenario_name: r.base.scenario_name,
                    decision: match r.base.decision { policies::features::policy_playground::dto::Decision::Allow => "Allow".into(), policies::features::policy_playground::dto::Decision::Deny => "Deny".into() },
                    reasons: r.base.diagnostics.reasons,
                    determining_policies: r.determining_policies,
                })
                .collect(),
            statistics,
        };

        // Metrics per scenario
        for r in &response.authorization_results {
            state.metrics.record_authorization(r.decision == "Allow");
        }

        return Ok(Json(response));
    }

    let result = state
        .policy_playground_uc
        .execute(&domain_req)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    let policy_validation = PolicyValidationApi {
        is_valid: result.policy_validation.is_valid,
        errors: result
            .policy_validation
            .errors
            .into_iter()
            .map(|e| ValidationErrorApi {
                message: e.message,
                policy_id: e.policy_id,
                line: e.line,
                column: e.column,
            })
            .collect(),
        warnings: result
            .policy_validation
            .warnings
            .into_iter()
            .map(|w| ValidationWarningApi {
                message: w.message,
                severity: match w.severity {
                    policies::features::policy_playground::dto::WarningSeverity::Low => "Low".into(),
                    policies::features::policy_playground::dto::WarningSeverity::Medium => "Medium".into(),
                    policies::features::policy_playground::dto::WarningSeverity::High => "High".into(),
                },
            })
            .collect(),
        policies_count: result.policy_validation.policies_count,
    };

    let schema_validation = SchemaValidationApi {
        is_valid: result.schema_validation.is_valid,
        errors: result.schema_validation.errors,
        entity_types_count: result.schema_validation.entity_types_count,
        actions_count: result.schema_validation.actions_count,
    };

    let statistics = EvaluationStatisticsApi {
        total_scenarios: result.statistics.total_scenarios,
        allow_count: result.statistics.allow_count,
        deny_count: result.statistics.deny_count,
        total_evaluation_time_us: result.statistics.total_evaluation_time_us,
        average_evaluation_time_us: result.statistics.average_evaluation_time_us,
    };

    let response = PlaygroundResponseApi {
        policy_validation,
        schema_validation,
        authorization_results: result
            .authorization_results
            .into_iter()
            .map(|r| PlaygroundAuthResultApi {
                scenario_name: r.scenario_name,
                decision: match r.decision { policies::features::policy_playground::dto::Decision::Allow => "Allow".into(), policies::features::policy_playground::dto::Decision::Deny => "Deny".into() },
                reasons: r.diagnostics.reasons,
                determining_policies: None,
            })
            .collect(),
        statistics,
    };

    // Metrics per scenario
    for r in &response.authorization_results {
        state.metrics.record_authorization(r.decision == "Allow");
    }

    Ok(Json(response))
}

/// Update a policy
///
/// Replaces an existing policy identified by {policy_id} with the provided Cedar policy content.
#[utoipa::path(
    put,
    path = "/api/v1/policies/{policy_id}",
    request_body = UpdatePolicyRequest,
    params(
        ("policy_id" = String, Path, description = "Unique identifier of the policy to update")
    ),
    responses(
        (status = 200, description = "Policy updated successfully", body = PolicyResponse),
        (status = 400, description = "Invalid request", body = ErrorResponse),
        (status = 404, description = "Policy not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn update_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
    Json(request): Json<UpdatePolicyRequest>,
) -> Result<Json<PolicyResponse>> {
    tracing::info!(policy_id = %policy_id, "Updating policy");

    if policy_id.trim().is_empty() {
        return Err(AppError::BadRequest("Policy ID cannot be empty".to_string()));
    }
    if request.policy_content.trim().is_empty() {
        return Err(AppError::BadRequest("Policy content cannot be empty".to_string()));
    }

    let cmd = policies::features::update_policy::dto::UpdatePolicyCommand::new(
        policy_id.clone(),
        request.policy_content.clone(),
    );

    let updated = state
        .update_policy_uc
        .execute(&cmd)
        .await
        .map_err(|e| match e {
            policies::features::update_policy::use_case::UpdatePolicyError::NotFound(_) => {
                AppError::NotFound(format!("Policy with ID '{}' not found", policy_id))
            }
            policies::features::update_policy::use_case::UpdatePolicyError::InvalidCommand(msg) => {
                AppError::BadRequest(msg)
            }
            policies::features::update_policy::use_case::UpdatePolicyError::ParseError(msg) => {
                AppError::BadRequest(format!("Parse error: {}", msg))
            }
            policies::features::update_policy::use_case::UpdatePolicyError::ValidationError(msg) => {
                AppError::BadRequest(format!("Validation failed: {}", msg))
            }
            policies::features::update_policy::use_case::UpdatePolicyError::Storage(msg) => {
                AppError::Internal(msg)
            }
        })?;

    state.metrics.record_policy_operation();

    let now = chrono::Utc::now().to_rfc3339();
    let response = PolicyResponse {
        id: updated.id().to_string(),
        name: updated.id().to_string(),
        description: None,
        policy_content: updated.to_string(),
        enabled: true,
        created_at: now.clone(),
        updated_at: now,
    };

    tracing::info!(policy_id = %response.id, "Policy updated successfully");
    Ok(Json(response))
}

/// Validate a policy without persisting
///
/// Validates a Cedar policy's syntax and semantics against the current schema without saving it.
#[utoipa::path(
    post,
    path = "/api/v1/policies/validate",
    request_body = ValidatePolicyRequest,
    responses(
        (status = 200, description = "Validation result", body = serde_json::Value),
        (status = 400, description = "Invalid request", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn validate_policy(
    State(state): State<Arc<AppState>>,
    Json(request): Json<ValidatePolicyRequest>,
) -> Result<Json<serde_json::Value>> {
    tracing::debug!("Validating policy content");

    let query = policies::features::validate_policy::dto::ValidatePolicyQuery::new(
        request.policy_content.clone(),
    );

    let result = state
        .validate_policy_uc
        .execute(&query)
        .await
        .map_err(|e| AppError::BadRequest(e.to_string()))?;

    state.metrics.record_policy_operation();

    Ok(Json(serde_json::json!({
        "is_valid": result.is_valid,
        "errors": result.errors,
        "warnings": result.warnings,
    })))
}

/// Delete a policy
///
/// Deletes a specific policy by its unique identifier.
#[utoipa::path(
    delete,
    path = "/api/v1/policies/{policy_id}",
    params(
        ("policy_id" = String, Path, description = "Unique identifier of the policy to delete")
    ),
    responses(
        (status = 200, description = "Policy deleted successfully"),
        (status = 400, description = "Invalid policy ID", body = ErrorResponse),
        (status = 404, description = "Policy not found", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn delete_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    tracing::info!(
        policy_id = %policy_id,
        "Deleting policy"
    );

    // Build command
    let cmd = policies::features::delete_policy::dto::DeletePolicyCommand::new(policy_id.clone());

    // Execute use case from AppState
    state
        .delete_policy_uc
        .execute(&cmd)
        .await
        .map_err(|e| match e {
            policies::features::delete_policy::use_case::DeletePolicyError::NotFound(_) => {
                AppError::NotFound(format!("Policy with ID '{}' not found", policy_id))
            }
            policies::features::delete_policy::use_case::DeletePolicyError::InvalidCommand(msg) => {
                AppError::BadRequest(msg)
            }
            policies::features::delete_policy::use_case::DeletePolicyError::Storage(msg) => {
                AppError::Internal(msg)
            }
        })?;

    // Record metrics
    state.metrics.record_policy_operation();

    tracing::info!(
        policy_id = %policy_id,
        "Policy deleted successfully"
    );

    Ok(Json(serde_json::json!({
        "message": "Policy deleted successfully",
        "policy_id": policy_id,
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}

/// Get a policy by ID
///
/// Retrieves a specific policy by its unique identifier.
#[utoipa::path(
    get,
    path = "/api/v1/policies/{policy_id}",
    params(
        ("policy_id" = String, Path, description = "Unique identifier of the policy")
    ),
    responses(
        (status = 200, description = "Policy found successfully", body = PolicyResponse),
        (status = 404, description = "Policy not found", body = ErrorResponse),
        (status = 400, description = "Invalid policy ID", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "policies"
)]
pub async fn get_policy(
    State(state): State<Arc<AppState>>,
    Path(policy_id): Path<String>,
) -> Result<Json<PolicyResponse>> {
    tracing::info!(
        policy_id = %policy_id,
        "Getting policy"
    );

    // Validate policy_id
    if policy_id.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Policy ID cannot be empty".to_string(),
        ));
    }

    // Build query
    let query = policies::features::get_policy::dto::GetPolicyQuery::new(policy_id.clone());

    // Execute use case from AppState
    let policy = state
        .get_policy_uc
        .execute(&query)
        .await
        .map_err(|e| match e {
            policies::features::get_policy::use_case::GetPolicyError::NotFound(_) => {
                AppError::NotFound(format!("Policy with ID '{}' not found", policy_id))
            }
            policies::features::get_policy::use_case::GetPolicyError::InvalidQuery(msg) => {
                AppError::BadRequest(msg)
            }
            policies::features::get_policy::use_case::GetPolicyError::Storage(msg) => {
                AppError::Internal(msg)
            }
        })?;

    // Record metrics
    state.metrics.record_policy_operation();

    // Convert Cedar Policy to PolicyResponse
    let now = chrono::Utc::now().to_rfc3339();
    let response = PolicyResponse {
        id: policy.id().to_string(),
        name: policy.id().to_string(), // Cedar policies don't have separate names
        description: None,
        policy_content: policy.to_string(),
        enabled: true,
        created_at: now.clone(),
        updated_at: now,
    };

    tracing::info!(
        policy_id = %policy_id,
        "Policy retrieved successfully"
    );

    Ok(Json(response))
}
