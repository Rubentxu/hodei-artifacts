//! The ports of the trust policy: where the decisions are written and where
//! the distributions are.
//!
//! The ports are narrow on purpose. The engine of the policy does not
//! know how a decision is stored, and the store does not know how a
//! decision is reached. What the store has to do is one thing well: keep
//! the decision as it was made, so that it can be read again later.

use async_trait::async_trait;

use crate::policy::TrustPolicy;
use crate::promotion::{DecisionId, PromotionDecision, PromotionRequest, PromotionTarget};

/// Why a decision could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecisionStoreError {
    /// The store is not available.
    #[error("the decision store is not available: {0}")]
    Unavailable(String),
    /// The decision is not there.
    #[error("the decision is not there")]
    NotFound,
}

/// The store of the promotion decisions.
///
/// A decision is written once and never changed. The store does not offer
/// a way to update a decision, because a decision that can be edited is
/// not a decision.
#[async_trait]
pub trait DecisionStore: Send + Sync {
    /// Writes a decision down.
    async fn put(&self, decision: &PromotionDecision) -> Result<(), DecisionStoreError>;

    /// Reads a decision by its identifier.
    async fn get(&self, id: &DecisionId) -> Result<Option<PromotionDecision>, DecisionStoreError>;

    /// Reads the decisions of a subject.
    async fn list_for_subject(
        &self,
        account: &str,
        subject: &str,
    ) -> Result<Vec<PromotionDecision>, DecisionStoreError>;

    /// Reads the decision of a correlation.
    async fn find_by_correlation(
        &self,
        correlation_id: &str,
    ) -> Result<Option<PromotionDecision>, DecisionStoreError>;
}

/// The loader of the policy that decides.
///
/// A policy that cannot be loaded is not a policy that refuses: it is an
/// operational failure, and the promotion has to tell the two apart. The
/// loader is a port so that the failure can be produced where it happens
/// --a store that is down, a set that was never activated-- and not
/// simulated in the caller.
#[async_trait]
pub trait PolicyLoader: Send + Sync {
    /// The policy set that is active for an identifier.
    async fn active_policy(&self, policy_id: &str) -> Result<TrustPolicy, DecisionStoreError>;
}

/// The destinations a promotion can go to.
///
/// A deployment that has no distribution says so, and the promotion
/// answers that the operation is not supported. It is the honest answer:
/// the alternative is a success that did not happen.
#[async_trait]
pub trait PromotionTargets: Send + Sync {
    /// The distribution of a subject, if there is one.
    async fn target_of(
        &self,
        account: &str,
        subject: &str,
    ) -> Result<Option<PromotionTarget>, DecisionStoreError>;
}

/// The subject of a promotion, as the request asks for it.
pub fn subject_of(request: &PromotionRequest) -> &str {
    &request.subject
}
