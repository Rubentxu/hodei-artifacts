use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalyzePoliciesRequest {
    pub policies: Vec<String>,
    pub schema: Option<String>,
    #[serde(default)]
    pub rules: Vec<AnalysisRule>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AnalysisRule {
    pub id: String,
    /// Example: "no_permit_without_mfa"
    pub kind: String,
    /// Optional data for rule (e.g., action name, resource type)
    #[serde(default)]
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyzePoliciesResponse {
    pub passed: bool,
    #[serde(default)]
    pub violations: Vec<RuleViolation>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuleViolation {
    pub rule_id: String,
    pub message: String,
}
