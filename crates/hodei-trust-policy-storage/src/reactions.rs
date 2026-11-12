//! The adapter that asks for a reevaluation.
//!
//! The runtime knows that a decision has to change. It does not know how
//! a decision is taken, what a policy is, or what a promotion is. This
//! adapter is the bridge: it builds the request, it asks the promotion
//! service, and it links the new decision to the one it replaces.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_reactive::reactions::{DecisionRequester, Reevaluation};
use hodei_trust_policy::context::TrustContext;
use hodei_trust_policy::gate::AuthorizationResult;
use hodei_trust_policy::ports::DecisionStore;
use hodei_trust_policy::promotion::{DecisionId, PromotionRequest};
use hodei_trust_policy::service::PromotionService;

/// Where the context of a subject comes from.
///
/// The context is the truth about the subject --the dimensions of its
/// capsule-- and the adapter asks for it instead of inventing it: a
/// reevaluation that decides with a context it made up is not a
/// reevaluation.
#[async_trait]
pub trait TrustContextSource: Send + Sync {
    /// The context of a subject, with the facts of the risk that are
    /// known about it.
    async fn context_of(
        &self,
        subject: &str,
        critical_reachable: u32,
        at: DateTime<Utc>,
    ) -> Result<TrustContext, String>;
}

/// The reevaluation over the real promotion service.
pub struct PromotionDecisionRequester {
    service: Arc<PromotionService>,
    store: Arc<dyn DecisionStore>,
    contexts: Arc<dyn TrustContextSource>,
    policy_id: String,
    distribution: String,
    principal: String,
}

impl PromotionDecisionRequester {
    /// The adapter with everything it needs to decide.
    pub fn new(
        service: Arc<PromotionService>,
        store: Arc<dyn DecisionStore>,
        contexts: Arc<dyn TrustContextSource>,
        policy_id: impl Into<String>,
        distribution: impl Into<String>,
        principal: impl Into<String>,
    ) -> Self {
        Self {
            service,
            store,
            contexts,
            policy_id: policy_id.into(),
            distribution: distribution.into(),
            principal: principal.into(),
        }
    }

    /// The account of a subject.
    ///
    /// The subject is an HRN and the HRN lleva la cuenta, asi que el
    /// adaptador no la inventa: la lee.
    fn the_account_of(subject: &str) -> String {
        subject.split(':').nth(4).unwrap_or_default().to_string()
    }
}

#[async_trait]
impl DecisionRequester for PromotionDecisionRequester {
    async fn request_reevaluation(&self, request: Reevaluation) -> Result<String, String> {
        let account = Self::the_account_of(&request.subject);
        if account.is_empty() {
            return Err(format!(
                "the subject {} does not say which account it belongs to",
                request.subject
            ));
        }

        // The context of the subject, with the facts of the risk.
        let context = self
            .contexts
            .context_of(
                &request.subject,
                request.advisories.len() as u32,
                request.at,
            )
            .await?;

        // The decision that is replaced, if there was one. The new
        // decision says which one it replaces: sin eso, una decision que
        // pasa de ALLOW a DENY no se puede explicar.
        let previous = self
            .store
            .list_for_subject(&account, &request.subject)
            .await
            .map_err(|error| error.to_string())?;
        let correlation = previous
            .first()
            .map(|decision| decision.id.as_str().to_string())
            .unwrap_or_else(|| format!("reevaluation:{}", request.subject));

        let id = DecisionId::new(format!(
            "dec-{}:{}",
            request.reason_label(),
            request.subject.replace(':', "-")
        ));

        let promotion = PromotionRequest::new(
            correlation,
            self.principal.clone(),
            account,
            request.subject.clone(),
            self.distribution.clone(),
            request.at,
        );

        // The authorization of the reevaluation is the system's own: the
        // decision is reevaluated because the truth changed, and not
        // because somebody asked.
        let outcome = self
            .service
            .request_promotion(
                id.clone(),
                promotion,
                AuthorizationResult::Permit,
                context,
                &self.policy_id,
            )
            .await
            .map_err(|error| error.to_string())?;

        // A promotion that is REFUSED is not a failure of the
        // reevaluation: the reevaluation is exactly the decision, and a
        // decision that denies is a decision that was taken. The only
        // failure is the one where there was no target and therefore no
        // decision at all.
        match outcome {
            hodei_trust_policy::promotion::PromotionOutcome::Failed(
                hodei_trust_policy::promotion::PromotionFailure::NoTarget,
            ) => Err(format!(
                "the reevaluation of {} did not produce a decision",
                request.subject
            )),
            _ => Ok(id.as_str().to_string()),
        }
    }
}

/// The name of a reason, for the identifier of the decision.
trait ReasonLabel {
    /// The name of the reason.
    fn reason_label(&self) -> &'static str;
}

impl ReasonLabel for Reevaluation {
    fn reason_label(&self) -> &'static str {
        match self.reason {
            hodei_reactive::reactions::ReasonCode::VulnerabilityImpacted => "vulnerability",
            hodei_reactive::reactions::ReasonCode::ExceptionExpired => "expired",
        }
    }
}

/// The context of a subject that has no capsule.
///
/// It is the honest answer when there is no evidence: a context with no
/// dimensions and the facts del riesgo. The policy that requires
/// dimensions will say that they are missing, and the decision will be
/// the one that says so.
pub struct EmptyContextSource;

#[async_trait]
impl TrustContextSource for EmptyContextSource {
    async fn context_of(
        &self,
        subject: &str,
        critical_reachable: u32,
        at: DateTime<Utc>,
    ) -> Result<TrustContext, String> {
        Ok(TrustContext {
            version: hodei_trust_policy::context::TRUST_CONTEXT_VERSION.to_string(),
            subject: hodei_trust_policy::context::SubjectFact {
                hrn: subject.to_string(),
                digest: None,
            },
            target: hodei_trust_policy::context::TargetEnvironment::Production,
            dimensions: Vec::new(),
            capsule_hash: None,
            risk: hodei_trust_policy::context::RiskFacts {
                critical_known: critical_reachable,
                critical_reachable,
                exposure: hodei_trust_policy::context::Exposure::Internal,
                vex_accepted: 0,
                established: critical_reachable > 0,
            },
            exceptions: Vec::new(),
            evidence_ids: Vec::new(),
            assembled_at: at,
        })
    }
}
