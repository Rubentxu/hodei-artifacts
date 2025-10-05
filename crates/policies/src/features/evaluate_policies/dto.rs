//! DTOs for the evaluate_policies feature
//!
//! This module defines the data transfer objects used for policy evaluation.

use serde::{Deserialize, Serialize};

/// Request to evaluate a set of policies against a principal, action, and resource
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatePoliciesRequest {
    /// Cedar policy documents to evaluate
    pub policies: Vec<String>,

    /// Principal EntityUid (e.g., "Iam::User::\"alice\"")
    pub principal: String,

    /// Action EntityUid (e.g., "Action::\"read\"")
    pub action: String,

    /// Resource EntityUid (e.g., "S3::Bucket::\"my-bucket\"")
    pub resource: String,

    /// Optional context data for the evaluation
    #[serde(default)]
    pub context: Option<std::collections::HashMap<String, serde_json::Value>>,

    /// Optional entities for the evaluation (principals, resources, etc.)
    #[serde(default)]
    pub entities: Vec<EntityDefinition>,
}

/// Entity definition for evaluation context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityDefinition {
    /// Entity UID (e.g., "Iam::User::\"alice\"")
    pub uid: String,

    /// Entity attributes
    #[serde(default)]
    pub attributes: std::collections::HashMap<String, serde_json::Value>,

    /// Parent entities (for hierarchies)
    #[serde(default)]
    pub parents: Vec<String>,
}

/// Response from policy evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatePoliciesResponse {
    /// Whether the request was allowed
    pub decision: Decision,

    /// Reason for the decision (e.g., which policy matched)
    pub reason: String,

    /// Diagnostics information (errors, warnings)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<EvaluationDiagnostics>,

    /// Evaluation time in microseconds
    pub evaluation_time_us: u64,
}

/// Decision result
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// Access allowed
    Allow,
    /// Access denied
    Deny,
}

/// Diagnostics from evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationDiagnostics {
    /// Errors encountered during evaluation
    #[serde(default)]
    pub errors: Vec<String>,

    /// Warnings encountered during evaluation
    #[serde(default)]
    pub warnings: Vec<String>,

    /// Policies that were satisfied (for debugging)
    #[serde(default)]
    pub satisfied_policies: Vec<String>,

    /// Policies that produced errors
    #[serde(default)]
    pub errored_policies: Vec<String>,
}

impl Default for EvaluationDiagnostics {
    fn default() -> Self {
        Self {
            errors: Vec::new(),
            warnings: Vec::new(),
            satisfied_policies: Vec::new(),
            errored_policies: Vec::new(),
        }
    }
}
