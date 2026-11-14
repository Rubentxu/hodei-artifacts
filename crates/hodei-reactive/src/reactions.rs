//! The reactions of the security domain.
//!
//! The runtime is the mechanism --suscribir, guardar, registrar, acotar--
//! and these are the reactions that use it: the one that answers to a new
//! vulnerability and the one that answers to an exception that expired.
//!
//! They talk to the domain through **ports**, so that the runtime does not
//! know what a case is and the reactions can be exercised without a graph.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::behavior::{Behavior, BehaviorError, Effect, GuardOutcome, Reaction};
use crate::events::{Event, EventKind, PayloadPredicate, Subscription};
use crate::intent::{
    Intent, IntentExecutor, IntentOrigin, Plan, PlanningBehavior, ReadScope, Receipt,
};

/// The subject that a vulnerability reaches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImpactedSubject {
    /// The subject.
    pub subject: String,
    /// The deployment where it runs.
    pub deployment: Option<String>,
    /// How far the dependency is: one is direct, more is transitive.
    pub depth: usize,
}

/// Where the blast radius of a vulnerability comes from.
#[async_trait]
pub trait BlastRadiusSource: Send + Sync {
    /// The subjects that the vulnerability reaches, of an account.
    async fn impacted(&self, account: &str, advisory: &str)
    -> Result<Vec<ImpactedSubject>, String>;
}

/// What the reaction does with the case of a vulnerability.
#[async_trait]
pub trait SecurityCaseSink: Send + Sync {
    /// The case of the vulnerability, opened or updated.
    ///
    /// It is **idempotent**: asking twice gives the same case, because a
    /// case that is opened twice for the same thing is two cases and
    /// nobody owns either.
    async fn open_or_update(
        &self,
        advisory: &str,
        impacted: &[ImpactedSubject],
        now: DateTime<Utc>,
    ) -> Result<String, String>;
}

/// What a reevaluation has to take into account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reevaluation {
    /// The subject whose decision is reevaluated.
    pub subject: String,
    /// Why it is reevaluated.
    pub reason: ReasonCode,
    /// The vulnerabilities that are known to reach it.
    ///
    /// They travel con la peticion porque son el hecho que cambia la
    /// decision: sin ellos, la reevaluacion no tendria con que ser
    /// distinta de la decision anterior.
    pub advisories: Vec<String>,
    /// The moment of the reevaluation.
    pub at: DateTime<Utc>,
}

impl Reevaluation {
    /// A reevaluation of a subject, and why.
    pub fn new(subject: impl Into<String>, reason: ReasonCode, at: DateTime<Utc>) -> Self {
        Self {
            subject: subject.into(),
            reason,
            advisories: Vec::new(),
            at,
        }
    }

    /// The reevaluation knows of a vulnerability.
    pub fn with_advisory(mut self, advisory: impl Into<String>) -> Self {
        self.advisories.push(advisory.into());
        self
    }
}

/// What the reaction does with the decision of a promotion.
#[async_trait]
pub trait DecisionRequester: Send + Sync {
    /// A decision is asked for, and the old one is superseded.
    ///
    /// The chain de decisiones es lo que permite explicar por que una
    /// promocion que era ALLOW paso a ser DENY: la nueva dice cual
    /// sustituyo a la anterior.
    async fn request_reevaluation(&self, request: Reevaluation) -> Result<String, String>;
}

/// Why a decision was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// A vulnerability that reaches the subject.
    VulnerabilityImpacted,
    /// An exception that expired.
    ExceptionExpired,
}

/// The reaction to a vulnerability that was accepted.
///
/// The chain is the one del hito: `CVE accepted -> impacted artifacts ->
/// reevaluation -> SecurityCase updated`. And the orden matters: primero
/// el radio de impacto, porque una vulnerabilidad que no toca nada no
/// reevalua nada.
pub struct VulnerabilityReaction {
    radius: Arc<dyn BlastRadiusSource>,
    cases: Arc<dyn SecurityCaseSink>,
    decisions: Arc<dyn DecisionRequester>,
}

impl VulnerabilityReaction {
    /// The reaction with its three ports.
    pub fn new(
        radius: Arc<dyn BlastRadiusSource>,
        cases: Arc<dyn SecurityCaseSink>,
        decisions: Arc<dyn DecisionRequester>,
    ) -> Self {
        Self {
            radius,
            cases,
            decisions,
        }
    }
}

#[async_trait]
impl Behavior for VulnerabilityReaction {
    fn name(&self) -> &str {
        "vulnerability_impact"
    }

    fn subscription(&self) -> Subscription {
        Subscription::to(EventKind::VulnerabilityAccepted).where_(PayloadPredicate::FieldExists {
            field: "advisory".to_string(),
        })
    }

    fn guard(&self, event: &Event) -> GuardOutcome {
        let Some(advisory) = event.payload.get("advisory").and_then(|v| v.as_str()) else {
            return GuardOutcome::Skip("the event does not name a vulnerability".to_string());
        };
        let Some(account) = event.payload.get("account").and_then(|v| v.as_str()) else {
            return GuardOutcome::Skip("the event does not name an account".to_string());
        };
        let _ = (advisory, account);
        GuardOutcome::Pass
    }

    async fn act(&self, event: &Event) -> Result<Reaction, BehaviorError> {
        let advisory = event
            .payload
            .get("advisory")
            .and_then(|value| value.as_str())
            .ok_or_else(|| BehaviorError::permanent("the event does not name a vulnerability"))?
            .to_string();
        let account = event
            .payload
            .get("account")
            .and_then(|value| value.as_str())
            .ok_or_else(|| BehaviorError::permanent("the event does not name an account"))?
            .to_string();

        // The blast radius: what the vulnerability reaches.
        let impacted = self
            .radius
            .impacted(&account, &advisory)
            .await
            .map_err(BehaviorError::transient)?;

        if impacted.is_empty() {
            // A vulnerability that touches nothing does not cause any
            // reevaluation: churning decisions about releases that are
            // not affected is noise, and noise is what makes people stop
            // reading the reports.
            return Ok(Reaction {
                events: Vec::new(),
                effects: Vec::new(),
            });
        }

        // The case, opened or updated.
        let case = self
            .cases
            .open_or_update(&advisory, &impacted, event.happened_at)
            .await
            .map_err(BehaviorError::transient)?;

        // And the reevaluation of what it reaches.
        let mut effects = Vec::new();
        let mut events = Vec::new();
        for subject in &impacted {
            let decision = self
                .decisions
                .request_reevaluation(
                    Reevaluation::new(
                        subject.subject.clone(),
                        ReasonCode::VulnerabilityImpacted,
                        event.happened_at,
                    )
                    .with_advisory(&advisory),
                )
                .await
                .map_err(BehaviorError::transient)?;

            effects.push(Effect::new(
                "decision",
                format!("decision:{advisory}:{}", subject.subject),
                serde_json::json!({
                    "subject": subject.subject,
                    "advisory": advisory,
                    "decision": decision,
                    "case": case,
                }),
            ));

            events.push(
                Event::new(
                    format!("evt-reevaluation:{}:{}", advisory, subject.subject),
                    EventKind::DecisionTaken,
                    serde_json::json!({
                        "subject": subject.subject,
                        "advisory": advisory,
                        "decision": decision,
                    }),
                    event.happened_at,
                )
                .caused_by(event.id.clone()),
            );
        }

        Ok(Reaction { events, effects })
    }
}

/// The reaction to an exception that expired.
///
/// An exception that expired **cannot** be applied: the reevaluation
/// happens with la excepcion ya sin vigor, y no con ella.
pub struct ExceptionExpiryReaction {
    decisions: Arc<dyn DecisionRequester>,
}

impl ExceptionExpiryReaction {
    /// The reaction with its port.
    pub fn new(decisions: Arc<dyn DecisionRequester>) -> Self {
        Self { decisions }
    }
}

#[async_trait]
impl Behavior for ExceptionExpiryReaction {
    fn name(&self) -> &str {
        "exception_expired"
    }

    fn subscription(&self) -> Subscription {
        Subscription::to(EventKind::ExceptionExpired)
    }

    fn guard(&self, event: &Event) -> GuardOutcome {
        match event
            .payload
            .get("subject")
            .and_then(|value| value.as_str())
        {
            Some(_) => GuardOutcome::Pass,
            None => GuardOutcome::Skip("the event does not name a subject".to_string()),
        }
    }

    async fn act(&self, event: &Event) -> Result<Reaction, BehaviorError> {
        let subject = event
            .payload
            .get("subject")
            .and_then(|value| value.as_str())
            .ok_or_else(|| BehaviorError::permanent("the event does not name a subject"))?
            .to_string();

        let decision = self
            .decisions
            .request_reevaluation(Reevaluation::new(
                subject.clone(),
                ReasonCode::ExceptionExpired,
                event.happened_at,
            ))
            .await
            .map_err(BehaviorError::transient)?;

        Ok(Reaction {
            events: vec![
                Event::new(
                    format!("evt-reevaluation-expired:{subject}"),
                    EventKind::DecisionTaken,
                    serde_json::json!({"subject": subject, "decision": decision}),
                    event.happened_at,
                )
                .caused_by(event.id.clone()),
            ],
            effects: vec![Effect::new(
                "decision",
                format!("decision:expired:{subject}"),
                serde_json::json!({"subject": subject, "decision": decision}),
            )],
        })
    }
}

/// The planned reaction to a vulnerability that was accepted.
///
/// It is the SAME chain as the legacy [`VulnerabilityReaction`], cut at
/// the seam: the planner READS the blast radius and SAYS intents, and it
/// does not touch a single sink. The case and the decisions belong to
/// the executor, which touches them after the intents are said and
/// claimed. What the planner cannot say is the decision itself: una
/// decision que todavia no se tomo no se dice, y un evento que inventa
/// su valor no es un evento.
///
/// A vulnerability that touches nothing says nothing: el ruido es lo que
/// hace que la gente deje de leer los informes.
pub struct PlannedVulnerabilityReaction {
    radius: Arc<dyn BlastRadiusSource>,
}

impl PlannedVulnerabilityReaction {
    /// The planner with its read port.
    pub fn new(radius: Arc<dyn BlastRadiusSource>) -> Self {
        Self { radius }
    }
}

#[async_trait]
impl PlanningBehavior for PlannedVulnerabilityReaction {
    fn name(&self) -> &str {
        "vulnerability_impact"
    }

    fn subscription(&self) -> Subscription {
        Subscription::to(EventKind::VulnerabilityAccepted).where_(PayloadPredicate::FieldExists {
            field: "advisory".to_string(),
        })
    }

    fn guard(&self, event: &Event) -> GuardOutcome {
        let Some(_advisory) = event.payload.get("advisory").and_then(|v| v.as_str()) else {
            return GuardOutcome::Skip("the event does not name a vulnerability".to_string());
        };
        let Some(_account) = event.payload.get("account").and_then(|v| v.as_str()) else {
            return GuardOutcome::Skip("the event does not name an account".to_string());
        };
        GuardOutcome::Pass
    }

    async fn plan(&self, event: &Event, scope: &ReadScope) -> Result<Plan, BehaviorError> {
        let advisory = event
            .payload
            .get("advisory")
            .and_then(|value| value.as_str())
            .ok_or_else(|| BehaviorError::permanent("the event does not name a vulnerability"))?
            .to_string();
        let account = event
            .payload
            .get("account")
            .and_then(|value| value.as_str())
            .ok_or_else(|| BehaviorError::permanent("the event does not name an account"))?
            .to_string();

        // The blast radius: what the vulnerability reaches. This is the
        // phase that MUST NOT touch: the radius is a READ, and what it
        // returns is what the intents are computed from.
        let impacted = self
            .radius
            .impacted(&account, &advisory)
            .await
            .map_err(BehaviorError::transient)?;

        if impacted.is_empty() {
            // A vulnerability that touches nothing does not cause any
            // reevaluation: no intents, no events, no noise.
            return Ok(Plan::default());
        }

        // The intent of the case: ONE case per advisory, said once.
        let case_intent = Intent::new(
            IntentOrigin::of(self.name(), event, &scope.origin.causation),
            scope.tenant.clone(),
            "security_case",
            format!("case:{advisory}"),
            scope.origin.event_id.clone(),
            serde_json::json!({
                "advisory": advisory,
                "at": scope.at.to_rfc3339(),
                "impacted": impacted,
            }),
        );
        let mut intents = vec![case_intent];

        // And one intent of reevaluation per subject the vulnerability
        // reaches.
        for entry in &impacted {
            intents.push(Intent::new(
                IntentOrigin::of(self.name(), event, &scope.origin.causation),
                scope.tenant.clone(),
                "decision_reevaluation",
                format!("decision:{advisory}:{}", entry.subject),
                scope.origin.event_id.clone(),
                serde_json::json!({
                    "subject": entry.subject,
                    "advisory": advisory,
                    "reason": ReasonCode::VulnerabilityImpacted,
                    "at": scope.at.to_rfc3339(),
                }),
            ));
        }

        Ok(Plan {
            intents,
            events: Vec::new(),
        })
    }
}

/// The planned reaction to an exception that expired.
///
/// An exception that expired **cannot** be applied: the reevaluation
/// happens with la excepcion ya sin vigor, y no con ella. The planner
/// needs no read: the subject is said by the event, and the decision
/// belongs to the executor.
pub struct PlannedExceptionExpiryReaction;

impl PlannedExceptionExpiryReaction {
    /// The planner, which needs no port to say what it wants.
    pub fn new() -> Self {
        Self
    }
}

impl Default for PlannedExceptionExpiryReaction {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl PlanningBehavior for PlannedExceptionExpiryReaction {
    fn name(&self) -> &str {
        "exception_expired"
    }

    fn subscription(&self) -> Subscription {
        Subscription::to(EventKind::ExceptionExpired)
    }

    fn guard(&self, event: &Event) -> GuardOutcome {
        match event
            .payload
            .get("subject")
            .and_then(|value| value.as_str())
        {
            Some(_) => GuardOutcome::Pass,
            None => GuardOutcome::Skip("the event does not name a subject".to_string()),
        }
    }

    async fn plan(&self, event: &Event, scope: &ReadScope) -> Result<Plan, BehaviorError> {
        let subject = event
            .payload
            .get("subject")
            .and_then(|value| value.as_str())
            .ok_or_else(|| BehaviorError::permanent("the event does not name a subject"))?
            .to_string();

        let intent = Intent::new(
            IntentOrigin::of(self.name(), event, &scope.origin.causation),
            scope.tenant.clone(),
            "decision_reevaluation",
            format!("decision:expired:{subject}"),
            scope.origin.event_id.clone(),
            serde_json::json!({
                "subject": subject,
                "reason": ReasonCode::ExceptionExpired,
                "at": scope.at.to_rfc3339(),
            }),
        );

        Ok(Plan {
            intents: vec![intent],
            events: Vec::new(),
        })
    }
}

/// What a `security_case` intent carries.
#[derive(Debug, Clone, serde::Deserialize)]
struct CaseEffect {
    /// The advisory the case is about.
    advisory: String,
    /// The moment of the observation the case was computed from.
    at: DateTime<Utc>,
    /// The subjects the advisory reaches.
    impacted: Vec<ImpactedSubject>,
}

/// What a `decision_reevaluation` intent carries.
#[derive(Debug, Clone, serde::Deserialize)]
struct DecisionEffect {
    /// The subject whose decision is reevaluated.
    subject: String,
    /// Why it is reevaluated.
    reason: ReasonCode,
    /// The advisories that are known to reach it, when there are any.
    #[serde(default)]
    advisory: Option<String>,
    /// The moment of the observation the intent was computed from.
    at: DateTime<Utc>,
}

/// The executor of the domain intents: the one that TOUCHES.
///
/// It is the second half of the seam the planners open: the case and the
/// reevaluation happen HERE, after the intents are said and claimed.
/// Before any write it validates what it can know by itself -- the
/// tenant of the intent is the tenant of the composition, una
/// reclamacion no es una identidad -- and the facts of the decision are
/// computed at apply time from the CURRENT truth, never from the
/// payload's memory of what they were.
pub struct DomainIntentExecutor {
    tenant: String,
    cases: Arc<dyn SecurityCaseSink>,
    decisions: Arc<dyn DecisionRequester>,
}

impl DomainIntentExecutor {
    /// The executor with its tenant and its two ports.
    pub fn new(
        tenant: impl Into<String>,
        cases: Arc<dyn SecurityCaseSink>,
        decisions: Arc<dyn DecisionRequester>,
    ) -> Self {
        Self {
            tenant: tenant.into(),
            cases,
            decisions,
        }
    }
}

#[async_trait]
impl IntentExecutor for DomainIntentExecutor {
    async fn apply(&self, intent: &Intent) -> Receipt {
        // The tenant: the intent says who it serves, and an intent of
        // another tenant is NOT ours to serve. It is a refusal without
        // retry, because no amount of retries makes the composition
        // right.
        if intent.tenant != self.tenant {
            return Receipt::Failed {
                reason: format!(
                    "the intent says tenant {} and this executor is {}",
                    intent.tenant, self.tenant
                ),
                retryable: false,
            };
        }

        match intent.kind.as_str() {
            "security_case" => {
                let Ok(effect) = serde_json::from_value::<CaseEffect>(intent.payload.clone())
                else {
                    return Receipt::Failed {
                        reason: format!(
                            "the intent {} does not carry a case this executor understands",
                            intent.identity
                        ),
                        retryable: false,
                    };
                };
                if effect.advisory.trim().is_empty() {
                    return Receipt::Failed {
                        reason: "the case does not say which advisory it is about".to_string(),
                        retryable: false,
                    };
                }
                match self
                    .cases
                    .open_or_update(&effect.advisory, &effect.impacted, effect.at)
                    .await
                {
                    Ok(_case) => Receipt::Applied,
                    Err(reason) => Receipt::Failed {
                        reason,
                        retryable: true,
                    },
                }
            }
            "decision_reevaluation" => {
                let Ok(effect) = serde_json::from_value::<DecisionEffect>(intent.payload.clone())
                else {
                    return Receipt::Failed {
                        reason: format!(
                            "the intent {} does not carry a reevaluation this executor understands",
                            intent.identity
                        ),
                        retryable: false,
                    };
                };
                if effect.subject.trim().is_empty() {
                    return Receipt::Failed {
                        reason: "the reevaluation does not say which subject it serves".to_string(),
                        retryable: false,
                    };
                }
                let mut request = Reevaluation::new(effect.subject, effect.reason, effect.at);
                if let Some(advisory) = effect.advisory {
                    request = request.with_advisory(advisory);
                }
                match self.decisions.request_reevaluation(request).await {
                    Ok(_decision) => Receipt::Applied,
                    Err(reason) => Receipt::Failed {
                        reason,
                        retryable: true,
                    },
                }
            }
            other => Receipt::Failed {
                reason: format!("the intent kind {other} is not one this executor touches"),
                retryable: false,
            },
        }
    }
}
