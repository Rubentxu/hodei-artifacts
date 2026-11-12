//! The gate of a promotion: two questions that were one.
//!
//! "Can this principal promote this?" and "does this artefact meet the
//! criteria?" were one question, and that was a mistake. Together they
//! make a failure of the criteria look like a failure of permissions, and
//! a decision that says "forbidden" without saying which of the two failed
//! cannot be explained to anybody.
//!
//! So they are separate. The authorization is decided by Cedar, which is
//! where Cedar is good: who may do what. The trust is decided by the
//! policy engine over a context that was assembled on purpose. And the
//! result says **which of the two decided**.

use serde::{Deserialize, Serialize};

use crate::context::TrustContext;
use crate::decision::{evaluate, Outcome, TrustDecision};
use crate::policy::TrustPolicy;
use crate::reasons::ReasonCode;

/// What the authorization decided.
///
/// The three states are told apart because a decision that cannot be made
/// is not the same as a decision that says no. `Indeterminate` is the
/// honest answer of a gate that could not reach one, and the default of
/// the gate is to refuse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthorizationResult {
    /// The principal may do this.
    Permit,
    /// The principal may not do this.
    Deny,
    /// The gate could not decide.
    Indeterminate,
}

impl AuthorizationResult {
    /// Whether the authorization lets the request through.
    pub fn permits(&self) -> bool {
        matches!(self, Self::Permit)
    }

    /// The reason code of this authorization result.
    pub fn reason(&self) -> ReasonCode {
        match self {
            Self::Permit => ReasonCode::AuthzPrincipalNotAllowed,
            Self::Deny => ReasonCode::AuthzPrincipalNotAllowed,
            Self::Indeterminate => ReasonCode::AuthzIndeterminate,
        }
    }
}

/// Which of the two halves decided the outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecidedBy {
    /// The authorization decided it.
    Authorization,
    /// The trust policy decided it.
    Trust,
    /// Both were satisfied, so neither refused.
    Neither,
}

/// The result of the gate.
///
/// It carries the two decisions and says which one refused. A reader never
/// has to guess whether a promotion was refused because the principal was
/// not allowed or because the SBOM was missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    /// How it came out.
    pub outcome: Outcome,
    /// Which of the two halves decided it.
    pub decided_by: DecidedBy,
    /// The decision of the authorization.
    pub authorization: AuthorizationResult,
    /// The decision of the trust policy.
    pub trust: TrustDecision,
}

impl GateResult {
    /// Whether the promotion may go ahead.
    pub fn advances(&self) -> bool {
        self.outcome.advances()
    }

    /// The reasons of the result, from the half that decided.
    pub fn reasons(&self) -> Vec<ReasonCode> {
        match self.decided_by {
            DecidedBy::Authorization => vec![self.authorization.reason()],
            DecidedBy::Trust => self.trust.reasons.clone(),
            DecidedBy::Neither => self.trust.reasons.clone(),
        }
    }
}

/// The gate of a promotion.
///
/// The authorization comes from outside -- from the Cedar authorizer of
/// the service -- and the trust comes from the policy engine. The gate
/// does not merge them into a single yes or no that loses the reason; it
/// runs both and says which one refused.
#[derive(Debug, Clone)]
pub struct PromotionGate {
    policy: TrustPolicy,
}

impl PromotionGate {
    /// A gate that measures against a policy set.
    pub fn new(policy: TrustPolicy) -> Self {
        Self { policy }
    }

    /// The policy the gate measures against.
    pub fn policy(&self) -> &TrustPolicy {
        &self.policy
    }

    /// Runs both halves.
    ///
    /// The order matters and it is the order of the questions: first
    /// whether the principal may, and then whether the artefact qualifies.
    /// The trust is **not** evaluated when the authorization refused, and
    /// that is on purpose: a decision that measured the trust of an
    /// artefact that nobody was allowed to touch would put the reasons of
    /// two different failures in the same trace.
    pub fn evaluate(
        &self,
        authorization: AuthorizationResult,
        context: &TrustContext,
    ) -> GateResult {
        if !authorization.permits() {
            return GateResult {
                outcome: Outcome::Deny,
                decided_by: DecidedBy::Authorization,
                authorization,
                trust: evaluate(context, &self.policy),
            };
        }

        let trust = evaluate(context, &self.policy);
        let decided_by = if trust.outcome == Outcome::Allow {
            DecidedBy::Neither
        } else {
            DecidedBy::Trust
        };

        GateResult {
            outcome: trust.outcome,
            decided_by,
            authorization,
            trust,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_indeterminate_authorization_is_not_a_permit() {
        assert!(!AuthorizationResult::Indeterminate.permits());
        assert!(!AuthorizationResult::Deny.permits());
        assert!(AuthorizationResult::Permit.permits());
    }
}
