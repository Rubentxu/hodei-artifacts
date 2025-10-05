use crate::domain::Hrn;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Request para evaluación de políticas
///
/// Utiliza referencias a HRN en lugar de traits para mantener la estructura serializable
#[derive(Debug, Clone, Serialize, Deserialize)]
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

pub trait ScpEvaluator: Send + Sync {
    fn evaluate_scps(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError>;
}

pub trait IamPolicyEvaluator: Send + Sync {
    fn evaluate_iam_policies(
        &self,
        request: EvaluationRequest,
    ) -> Result<EvaluationDecision, AuthorizationError>;
}
