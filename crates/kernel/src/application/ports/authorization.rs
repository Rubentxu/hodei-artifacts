use crate::domain::Hrn;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Request for policy evaluation
///
/// Request for policy evaluation
///
/// Holds references to HRNs to avoid excessive cloning in the hot path
/// of authorization, where multiple policies are evaluated per request.
#[derive(Debug, Clone)]
pub struct EvaluationRequest {
    pub principal_hrn: Hrn,
    pub action_name: String,
    pub resource_hrn: Hrn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationDecision {
    pub principal_hrn: Hrn,
    pub action_name: String,
    pub resource_hrn: Hrn,
    pub decision: bool,
    pub reason: String,
}

#[derive(Debug, Error)]
pub enum AuthorizationError {
    #[error("Evaluation failed: {0}")]
    EvaluationFailed(String),
    #[error("Policy not found")]
    PolicyNotFound,
    #[error("Invalid policy format")]
    InvalidPolicyFormat,
}

#[async_trait]
pub trait ScpEvaluator: Send + Sync {
    async fn evaluate_scps(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError>;
}

#[async_trait]
pub trait IamPolicyEvaluator: Send + Sync {
    async fn evaluate_iam_policies(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError>;
}
