//! The ports of the trust policy: where the decisions are written and where
//! the distributions are.
//!
//! The ports are narrow on purpose. The engine of the policy does not
//! know how a decision is stored, and the store does not know how a
//! decision is reached. What the store has to do is one thing well: keep
//! the decision as it was made, so that it can be read again later.

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::lifecycle::{
    validate_policy, Actors, Head, LifecycleError, LifecycleEvent, PolicyRevision,
};
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

/// The persistent lifecycle of policy sets: revisions, the head that
/// decides, and the audit events, one set per account.
///
/// It is the port of [`crate::lifecycle::PolicyLifecycle`] with the same
/// semantics, made durable and tenant-scoped: every call names the
/// `account` it acts for, and nothing of one account is readable or
/// writable through another. Revisions are immutable; the head only moves
/// through a compare-and-set on the revision the caller expects, and a lost
/// race is [`LifecycleError::StaleHead`], never an overwrite.
#[async_trait]
pub trait LifecycleStore: Send + Sync {
    /// Saves a policy as a new immutable revision, without activating it.
    async fn save(
        &self,
        account: &str,
        policy: TrustPolicy,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<PolicyRevision, LifecycleError>;

    /// Activates a saved revision if `expected_head` is the current head
    /// (`None` meaning "never activated").
    #[allow(clippy::too_many_arguments)]
    async fn activate(
        &self,
        account: &str,
        policy_id: &str,
        sequence: u64,
        expected_head: Option<u64>,
        approver: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Head, LifecycleError>;

    /// Rolls back to an older revision as a **new** revision, under the same
    /// compare-and-set as an activation.
    #[allow(clippy::too_many_arguments)]
    async fn rollback(
        &self,
        account: &str,
        policy_id: &str,
        target: u64,
        expected_head: Option<u64>,
        actors: &Actors,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Head, LifecycleError>;

    /// One revision of a policy set, if it exists.
    async fn revision(
        &self,
        account: &str,
        policy_id: &str,
        sequence: u64,
    ) -> Result<Option<PolicyRevision>, LifecycleError>;

    /// The whole history of a policy set, oldest first.
    async fn history(
        &self,
        account: &str,
        policy_id: &str,
    ) -> Result<Vec<PolicyRevision>, LifecycleError>;

    /// A bounded window of the history: at most `limit` revisions after
    /// skipping `offset`, oldest first (a stable order by sequence).
    async fn history_page(
        &self,
        account: &str,
        policy_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<PolicyRevision>, LifecycleError>;

    /// A bounded window of the audit events: at most `limit` after
    /// skipping `offset`, in the order they happened.
    async fn events_page(
        &self,
        account: &str,
        policy_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<LifecycleEvent>, LifecycleError>;

    /// The head that decides for a policy set, if it was ever activated.
    async fn head(&self, account: &str, policy_id: &str) -> Result<Option<Head>, LifecycleError>;

    /// The audit events of a policy set, in the order they happened.
    async fn events(
        &self,
        account: &str,
        policy_id: &str,
    ) -> Result<Vec<LifecycleEvent>, LifecycleError>;

    /// Saves a new revision and activates it, under the same CAS the
    /// separate calls use. The head is checked before the save so a stale
    /// caller does not leave a revision behind; it is not one transaction:
    /// the revision it saved stays in the history if the activation loses.
    #[allow(clippy::too_many_arguments)]
    async fn save_and_activate(
        &self,
        account: &str,
        policy: TrustPolicy,
        editor: &str,
        approver: &str,
        expected_head: Option<u64>,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<Head, LifecycleError> {
        validate_policy(&policy)?;
        let policy_id = policy.id.clone();
        let current = self
            .head(account, &policy_id)
            .await?
            .map(|head| head.sequence);
        if current != expected_head {
            return Err(LifecycleError::StaleHead(
                policy_id,
                expected_head.unwrap_or(0),
                current.unwrap_or(0),
            ));
        }
        let revision = self.save(account, policy, editor, reason, at).await?;
        self.activate(
            account,
            &policy_id,
            revision.sequence,
            expected_head,
            approver,
            reason,
            at,
        )
        .await
    }
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
