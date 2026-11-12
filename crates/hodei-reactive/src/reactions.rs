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
