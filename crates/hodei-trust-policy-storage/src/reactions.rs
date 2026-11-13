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
    /// adaptador no la inventa ni la busca a cuchillo: la lee del parser
    /// canonico. Un split de cadena es otro parser, y otro parser es otra
    /// gramatica que nadie acordo.
    fn the_account_of(subject: &str) -> Result<String, String> {
        kernel::Hrn::from_string(subject)
            .map(|one| one.account_id().to_string())
            .ok_or_else(|| format!("the subject {subject} is not a canonical HRN"))
    }
}

#[async_trait]
impl DecisionRequester for PromotionDecisionRequester {
    async fn request_reevaluation(&self, request: Reevaluation) -> Result<String, String> {
        let account = Self::the_account_of(&request.subject)?;

        // The context of the subject, with the facts of the risk.
        let context = self
            .contexts
            .context_of(
                &request.subject,
                request.advisories.len() as u32,
                request.at,
            )
            .await?;

        // The context is about THE SUBJECT of the reevaluation, and not
        // about the one the source felt like answering: si el contexto no
        // nombra al sujeto, la reevaluacion decide sobre otro, y una
        // decision sobre otro no es una decision.
        if context.subject.hrn != request.subject {
            return Err(format!(
                "the context returned is for {} and not for the subject {}",
                context.subject.hrn, request.subject
            ));
        }

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

#[cfg(test)]
mod the_account_of_tests {
    use super::PromotionDecisionRequester;

    /// Un HRN canonico v2, con region, dice la cuenta en su sitio.
    #[test]
    fn a_canonical_hrn_with_region_says_its_account() {
        let account = PromotionDecisionRequester::the_account_of(
            "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0",
        )
        .expect("un HRN canonico");
        assert_eq!(account, "acme");
    }

    /// Un alias v1, sin region, es el mismo HRN y dice la misma cuenta.
    #[test]
    fn a_v1_alias_without_region_says_its_account_too() {
        let account = PromotionDecisionRequester::the_account_of(
            "hrn:hodei:artifacts::acme:Artifact/app-1.0.0",
        )
        .expect("un HRN canonico");
        assert_eq!(account, "acme");
    }

    /// Lo que no es un HRN canonico no dice cuenta alguna: es un error,
    /// no una cadena vacia que el flujo ignora.
    #[test]
    fn a_subject_that_is_not_an_hrn_is_an_error() {
        for impostor in ["", "app-1.0.0", "hrn:", "hrn:hodei"] {
            let outcome = PromotionDecisionRequester::the_account_of(impostor);
            assert!(
                outcome.is_err(),
                "{impostor:?} no es un HRN canonico: {outcome:?}"
            );
        }
    }
}
