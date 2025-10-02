use serde::{Deserialize, Serialize};

use crate::features::policy_playground::dto as base;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TracedPlaygroundOptions {
    #[serde(default)]
    pub include_policy_traces: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TracedAuthorizationResult {
    pub base: base::AuthorizationResult,
    pub determining_policies: Option<Vec<String>>, // None when not requested or unavailable
    pub evaluated_policies: Option<Vec<base::PolicyEvaluation>>, // idem
}

#[derive(Debug, Clone, Serialize)]
pub struct TracedPlaygroundResponse {
    pub policy_validation: base::PolicyValidationResult,
    pub schema_validation: base::SchemaValidationResult,
    pub authorization_results: Vec<TracedAuthorizationResult>,
    pub statistics: base::EvaluationStatistics,
}
