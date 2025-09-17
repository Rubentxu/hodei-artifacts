//! Data transfer objects for policy evaluation feature

use crate::domain::context::PolicyEvaluationContext;
use crate::domain::decision::PolicyDecision;
use serde::{Deserialize, Serialize};
use shared::hrn::PolicyId;
use std::collections::HashMap;

/// Request to evaluate a policy against a context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatePolicyRequest {
    /// The policy ID to evaluate
    pub policy_id: PolicyId,
    /// The evaluation context containing principal, action, resource, and environment
    pub context: PolicyEvaluationContext,
}

/// Response from policy evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatePolicyResponse {
    /// The policy decision result
    pub decision: PolicyDecision,
    /// The policy ID that was evaluated
    pub policy_id: PolicyId,
    /// Evaluation timestamp
    pub evaluated_at: chrono::DateTime<chrono::Utc>,
    /// Performance metrics
    pub metrics: EvaluationMetrics,
}

/// Performance metrics for policy evaluation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationMetrics {
    /// Time taken for evaluation in milliseconds
    pub evaluation_time_ms: u64,
    /// Number of policies evaluated
    pub policies_evaluated: u32,
    /// Cache hit rate (0.0 to 1.0)
    pub cache_hit_rate: f64,
    /// Memory usage in bytes
    pub memory_usage_bytes: u64,
}

/// Batch policy evaluation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEvaluatePolicyRequest {
    /// List of policy evaluation requests
    pub requests: Vec<EvaluatePolicyRequest>,
    /// Whether to evaluate in parallel
    pub parallel: bool,
}

/// Batch policy evaluation response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEvaluatePolicyResponse {
    /// Individual evaluation responses
    pub responses: Vec<EvaluatePolicyResponse>,
    /// Batch-level metrics
    pub batch_metrics: BatchEvaluationMetrics,
}

/// Batch evaluation metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchEvaluationMetrics {
    /// Total time for batch evaluation
    pub total_time_ms: u64,
    /// Number of successful evaluations
    pub successful_evaluations: u32,
    /// Number of failed evaluations
    pub failed_evaluations: u32,
    /// Average evaluation time
    pub average_time_ms: f64,
}

/// Query for finding applicable policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindApplicablePoliciesQuery {
    /// The principal HRN
    pub principal: String,
    /// The action being performed
    pub action: String,
    /// The resource HRN
    pub resource: String,
    /// Optional organization context
    pub organization_id: Option<String>,
    /// Optional environment context
    pub environment: Option<HashMap<String, String>>,
}

/// Response containing applicable policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindApplicablePoliciesResponse {
    /// List of applicable policy IDs
    pub policy_ids: Vec<PolicyId>,
    /// Search metrics
    pub search_metrics: SearchMetrics,
}

/// Search metrics for policy lookup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMetrics {
    /// Time taken for search in milliseconds
    pub search_time_ms: u64,
    /// Number of policies scanned
    pub policies_scanned: u32,
    /// Number of policies matched
    pub policies_matched: u32,
}