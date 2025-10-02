use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaygroundRequest {
    pub policies: Vec<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub entities: Vec<EntityDefinition>,
    pub authorization_requests: Vec<AuthorizationScenario>,
    pub options: Option<PlaygroundOptions>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EntityDefinition {
    pub uid: String,
    pub attributes: HashMap<String, serde_json::Value>,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthorizationScenario {
    pub name: String,
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub context: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PlaygroundOptions {
    pub include_diagnostics: bool,
}

impl Default for PlaygroundOptions {
    fn default() -> Self {
        Self { include_diagnostics: true }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlaygroundResponse {
    pub policy_validation: PolicyValidationResult,
    pub schema_validation: SchemaValidationResult,
    pub authorization_results: Vec<AuthorizationResult>,
    pub statistics: EvaluationStatistics,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyValidationResult {
    pub is_valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<ValidationWarning>,
    pub policies_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchemaValidationResult {
    pub is_valid: bool,
    pub errors: Vec<String>,
    pub entity_types_count: usize,
    pub actions_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationResult {
    pub scenario_name: String,
    pub decision: Decision,
    pub determining_policies: Vec<String>,
    pub evaluated_policies: Vec<PolicyEvaluation>,
    pub diagnostics: AuthorizationDiagnostics,
    pub evaluation_time_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PolicyEvaluation {
    pub policy_id: String,
    pub result: PolicyResult,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub enum PolicyResult {
    Permit,
    Forbid,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizationDiagnostics {
    pub reasons: Vec<String>,
    pub errors: Vec<String>,
    pub info: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvaluationStatistics {
    pub total_scenarios: usize,
    pub allow_count: usize,
    pub deny_count: usize,
    pub total_evaluation_time_us: u64,
    pub average_evaluation_time_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub message: String,
    pub policy_id: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationWarning {
    pub message: String,
    pub severity: WarningSeverity,
}

#[derive(Debug, Clone, Serialize)]
pub enum WarningSeverity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
pub enum Decision { Allow, Deny }
