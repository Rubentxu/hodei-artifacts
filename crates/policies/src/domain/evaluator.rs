use crate::domain::{context::PolicyEvaluationContext, decision::PolicyDecision};

/// Evaluador de políticas. La implementación concreta integrará Cedar.
pub trait PolicyEvaluator {
    fn evaluate(&self, ctx: &PolicyEvaluationContext) -> PolicyDecision;
}
