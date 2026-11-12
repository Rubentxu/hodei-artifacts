//! The promotion service: the use case that goes with the domain.
//!
//! This is where the two halves meet and where the decision is written
//! down. The order is the order of the questions:
//!
//! 1. Is there anywhere to promote to? If there is not, the answer is
//!    that the operation is not supported, and the reason is written down
//!    so that the absence is visible.
//! 2. May this principal do it? The authorization is Cedar's.
//! 3. Does the artefact qualify? The trust policy's.
//! 4. The decision is written down, with everything that took part in it.
//!
//! And the decision is written down **whatever the outcome**. A refusal
//! that leaves no trace is a refusal that can be argued about.

use chrono::{DateTime, Utc};
use std::sync::Arc;

use crate::context::TrustContext;
use crate::decision::{Outcome, TrustDecision};
use crate::gate::{AuthorizationResult, DecidedBy, PromotionGate};
use crate::ports::{DecisionStore, PolicyLoader, PromotionTargets};
use crate::promotion::{
    DecisionId, PromotionDecision, PromotionFailure, PromotionOutcome, PromotionRequest,
};
use crate::reasons::ReasonCode;

/// The promotion service.
pub struct PromotionService {
    store: Arc<dyn DecisionStore>,
    targets: Arc<dyn PromotionTargets>,
    policies: Arc<dyn PolicyLoader>,
}

impl PromotionService {
    /// A service over a store, a set of destinations and a loader of the
    /// policy that decides.
    pub fn new(
        store: Arc<dyn DecisionStore>,
        targets: Arc<dyn PromotionTargets>,
        policies: Arc<dyn PolicyLoader>,
    ) -> Self {
        Self {
            store,
            targets,
            policies,
        }
    }

    /// Asks for a promotion and writes the decision down.
    ///
    /// The decision is written whatever came out, and the outcome says
    /// whether the promotion had a real effect.
    pub async fn request_promotion(
        &self,
        id: DecisionId,
        request: PromotionRequest,
        authorization: AuthorizationResult,
        context: TrustContext,
        policy_id: &str,
    ) -> Result<PromotionOutcome, crate::ports::DecisionStoreError> {
        let target = self
            .targets
            .target_of(&request.account, &request.subject)
            .await?;

        let decided_at = context.assembled_at;

        // The policy that decides is LOADED, and it may fail to load. A
        // policy that cannot be loaded is not a policy that refuses: it is
        // an operational failure, and the promotion tells the two apart.
        // The failure is closed -- nothing advances -- and the decision is
        // written down all the same, so that the absence is visible.
        let policy = match self.policies.active_policy(policy_id).await {
            Ok(policy) => policy,
            Err(_) => {
                let decision = PromotionDecision {
                    id,
                    request,
                    outcome: Outcome::Indeterminate,
                    decided_by: DecidedBy::Trust,
                    authorization,
                    trust: TrustDecision {
                        outcome: Outcome::Indeterminate,
                        reasons: vec![ReasonCode::PolicyUnavailable],
                        requirements: Vec::new(),
                        context_version: context.version.clone(),
                        policy_id: policy_id.to_string(),
                        policy_version: "unloaded".to_string(),
                        schema_version: crate::POLICY_SCHEMA_VERSION.to_string(),
                        capsule_hash: context.capsule_hash.clone(),
                        evidence_ids: context.evidence_ids.clone(),
                        decided_at,
                    },
                    context,
                    reasons: vec![ReasonCode::PolicyUnavailable],
                    applied: false,
                    decided_at,
                };
                self.store.put(&decision).await?;
                return Ok(PromotionOutcome::Failed(PromotionFailure::Refused));
            }
        };

        let gate = PromotionGate::new(policy);
        let result = gate.evaluate(authorization.clone(), &context);

        // The outcome of the promotion is the outcome of the gate, with
        // one adjustment: a criterion that could not be looked at because
        // a mandatory input was not there is **indeterminate**, and not a
        // refusal. The two are told apart because they lead to different
        // places: one waits for an approval and the other waits for the
        // evidence.
        let outcome = match result.outcome {
            Outcome::Deny
                if result
                    .trust
                    .reasons
                    .iter()
                    .any(|reason| reason.is_absence()) =>
            {
                Outcome::Indeterminate
            }
            other => other,
        };

        let applied = outcome.advances() && target.is_some();

        let reasons = result.reasons();
        let decided_by = result.decided_by;
        let trust = result.trust;

        let decision = PromotionDecision {
            id,
            request,
            outcome,
            decided_by,
            authorization,
            trust,
            context,
            reasons,
            applied,
            decided_at,
        };

        self.store.put(&decision).await?;

        Ok(if applied {
            PromotionOutcome::Promoted
        } else if target.is_none() {
            // There was nowhere to go. The decision is written down, and
            // the answer is that the operation is not supported.
            PromotionOutcome::Failed(PromotionFailure::NoTarget)
        } else {
            PromotionOutcome::Failed(PromotionFailure::Refused)
        })
    }

    /// Reads a decision by its identifier.
    pub async fn decision(
        &self,
        id: &DecisionId,
    ) -> Result<Option<PromotionDecision>, crate::ports::DecisionStoreError> {
        self.store.get(id).await
    }

    /// Reads the decisions of a subject.
    pub async fn decisions_of(
        &self,
        account: &str,
        subject: &str,
    ) -> Result<Vec<PromotionDecision>, crate::ports::DecisionStoreError> {
        self.store.list_for_subject(account, subject).await
    }
}

/// The trust decision that a gate reached, for the trace.
pub fn trust_of(decision: &PromotionDecision) -> &TrustDecision {
    &decision.trust
}

/// The reason of a promotion that did not happen because nothing was
/// missing and the principal was not allowed.
pub fn the_authorization_reason() -> ReasonCode {
    ReasonCode::AuthzPrincipalNotAllowed
}

/// The moment a decision was made.
pub fn decided_at(decision: &PromotionDecision) -> DateTime<Utc> {
    decision.decided_at
}

/// Which half decided, for the trace.
pub fn decided_by(decision: &PromotionDecision) -> DecidedBy {
    decision.decided_by
}
