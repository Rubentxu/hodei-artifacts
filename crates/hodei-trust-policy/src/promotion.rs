//! The promotion: what is asked, what is decided, and where it goes.
//!
//! A promotion is a **request** and a **decision**, and the decision is
//! written down. It says who decided, with which policy, over which
//! evidence, and with which result -- and it keeps saying so after the
//! promotion happened, because a flag that says "promoted" without history
//! is not a decision but an anecdote.
//!
//! The promotion is only declared when there is a **real effect on a
//! distribution**. Without a destination there is nothing to promote to,
//! and the answer is that the operation is not supported -- never a
//! success that did not happen.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::context::TrustContext;
use crate::decision::{Outcome, TrustDecision};
use crate::reasons::ReasonCode;

/// The identifier of a decision.
///
/// It is a type and not a string, because a decision that is read by the
/// wrong identifier is not the decision anybody meant.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DecisionId(String);

impl DecisionId {
    /// The identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The identifier as it reads.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for DecisionId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// What a promotion asks for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionRequest {
    /// The correlation of the request, which ties the decision to the
    /// conversation it came from.
    pub correlation_id: String,
    /// The principal that asks.
    pub principal: String,
    /// The account the promotion happens in.
    pub account: String,
    /// The subject that is promoted.
    pub subject: String,
    /// The distribution it goes to.
    pub distribution: String,
    /// The moment the request was made.
    pub requested_at: DateTime<Utc>,
}

impl PromotionRequest {
    /// A request to promote a subject to a distribution.
    pub fn new(
        correlation_id: impl Into<String>,
        principal: impl Into<String>,
        account: impl Into<String>,
        subject: impl Into<String>,
        distribution: impl Into<String>,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            correlation_id: correlation_id.into(),
            principal: principal.into(),
            account: account.into(),
            subject: subject.into(),
            distribution: distribution.into(),
            requested_at,
        }
    }
}

/// What was decided, and by whom.
///
/// This is the thing that is written down. It holds the decision of the
/// trust, the decision of the authorization, the context that was measured,
/// and the outcome of the promotion, so that a reader can see the whole
/// of it without asking the system to recompute anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromotionDecision {
    /// The identifier of the decision.
    pub id: DecisionId,
    /// The request it answers.
    pub request: PromotionRequest,
    /// The outcome of the promotion.
    pub outcome: Outcome,
    /// Which of the two halves decided it.
    pub decided_by: crate::gate::DecidedBy,
    /// The result of the authorization.
    pub authorization: crate::gate::AuthorizationResult,
    /// The decision of the trust.
    pub trust: TrustDecision,
    /// The context that was measured.
    pub context: TrustContext,
    /// The reasons of the outcome.
    pub reasons: Vec<ReasonCode>,
    /// Whether the promotion had a real effect on a distribution.
    pub applied: bool,
    /// The moment the decision was made.
    pub decided_at: DateTime<Utc>,
}

impl PromotionDecision {
    /// Whether the promotion went ahead.
    pub fn advanced(&self) -> bool {
        self.outcome.advances() && self.applied
    }

    /// The identifier of the correlation of the decision.
    pub fn correlation_id(&self) -> &str {
        &self.request.correlation_id
    }
}

/// Why a promotion could not be carried out.
///
/// The distinction that matters: a promotion that was **refused** is a
/// decision, and it is written down. A promotion that had **nowhere to go**
/// is not a decision at all: the operation is not supported, and saying
/// "not supported" is more honest than a success that did not happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromotionFailure {
    /// There is no distribution to promote to.
    NoTarget,
    /// The decision refused the promotion.
    Refused,
}

impl PromotionFailure {
    /// The name of the failure.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NoTarget => "not_supported",
            Self::Refused => "refused",
        }
    }
}

/// The outcome of a promotion that was carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotionOutcome {
    /// It went ahead.
    Promoted,
    /// It did not, and this is why.
    Failed(PromotionFailure),
}

impl PromotionOutcome {
    /// Whether the promotion went ahead.
    pub fn promoted(&self) -> bool {
        matches!(self, Self::Promoted)
    }
}

/// Where a promotion goes.
///
/// The port is what says whether there is a destination at all. A
/// deployment that has no distribution configured says so, and the
/// promotion answers that it is not supported -- and the decision is
/// written down as such, so that the absence is visible and not silent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromotionTarget {
    /// The distribution.
    pub distribution: String,
    /// The account it belongs to.
    pub account: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_promotion_without_a_target_is_not_a_success() {
        assert!(!PromotionOutcome::Failed(PromotionFailure::NoTarget).promoted());
        assert!(PromotionOutcome::Promoted.promoted());
        assert_eq!(
            PromotionFailure::NoTarget.as_str(),
            "not_supported",
            "and it says that the operation is not supported"
        );
    }
}
