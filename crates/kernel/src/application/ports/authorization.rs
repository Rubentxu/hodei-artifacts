use crate::domain::Hrn;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Request para evaluación de políticas
///
/// Request para evaluación de políticas
///
/// Contiene referencias a HRN para evitar clonaciones excesivas en el hot path
/// de autorización, donde se evalúan múltiples políticas por request.
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
