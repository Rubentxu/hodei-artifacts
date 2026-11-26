//! The investigator: the one that asks, checks and publishes.
//!
//! The flow is that of an investigation that can be audited:
//!
//! 1. It **retrieves** within a scope, and what is retrieved is
//!    **checked** against the scope again, because a retriever that
//!    makes a mistake cannot let anything in.
//! 2. It **asks** the model with the delimited context, declared
//!    as data.
//! 3. The answer is **validated**: it must be structured, and each
//!    claim must cite evidence that really resolved.
//! 4. A claim that is not valid is **not published**: the entire
//!    answer is rejected, because a half answer is worse than none.
//! 5. What the model proposes is **not executed**: it is recorded and waits for the
//!    approval of a person.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use kernel::Hrn;

use crate::claim::{AIClaim, ClaimError, ClaimKind, InvestigationTrace, check_the_claim};
use crate::guard::{ActionKind, CapabilityGuard, ProposedAction};
use crate::model::{ModelProvider, PROMPT_VERSION, the_prompt};
use crate::report::{InvestigationReport, build_report};
use crate::scope::{
    RetrievalError, RetrievalScope, RetrievedContext, ScopeViolation, ScopedFact, check_the_scope,
};

/// Where the evidence is retrieved from.
///
/// The retriever is the authorization BORDER: it authorizes BEFORE reading,
/// and a denial is explicit, not an empty answer.
#[async_trait::async_trait]
pub trait EvidenceRetriever: Send + Sync {
    /// The subject's subgraph, within the scope and the budget, with
    /// its coverage: how much of what exists was delivered.
    async fn retrieve(&self, scope: &RetrievalScope) -> Result<RetrievedContext, RetrievalError>;
}

/// Where the reachability claims come from.
///
/// Reachability is a fact of the code graph, and arrives by its
/// own path: it is not artifact evidence, it is symbol evidence.
/// That is why it has its own port, and why the investigator can investigate a
/// case without reachability (and then says nothing about it).
#[async_trait::async_trait]
pub trait ReachabilityFacts: Send + Sync {
    /// The reachability facts of the subject.
    async fn facts_of(&self, subject: &Hrn) -> Result<Vec<ScopedFact>, String>;
}

/// Why the investigation could not be carried out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvestigationError {
    /// The asker cannot read what they request. The door is closed
    /// BEFORE retrieving anything.
    NotAuthorized { actor: String, resource: String },
    /// The world changed while being read: the read is obsolete and is not
    /// presented as a consistent snapshot.
    Stale(String),
    /// The retrieval went outside the scope.
    OutOfScope(ScopeViolation),
    /// The model's answer could not be read.
    Unreadable(String),
    /// A claim was not valid.
    InvalidClaim(ClaimError),
    /// The model did not answer.
    NoAnswer(String),
}

impl std::fmt::Display for InvestigationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAuthorized { actor, resource } => {
                write!(formatter, "{actor} is not allowed to read {resource}")
            }
            Self::Stale(reason) => write!(formatter, "the reads are stale: {reason}"),
            Self::OutOfScope(violation) => {
                write!(formatter, "the retrieval left the scope: {violation}")
            }
            Self::Unreadable(reason) => {
                write!(formatter, "the answer could not be read: {reason}")
            }
            Self::InvalidClaim(error) => {
                write!(formatter, "a claim is not valid: {error}")
            }
            Self::NoAnswer(reason) => write!(formatter, "there is no answer: {reason}"),
        }
    }
}

/// What came out of an investigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Investigation {
    /// The valid claims.
    pub claims: Vec<AIClaim>,
    /// The trace: who, with what prompt, over what context and when.
    pub trace: InvestigationTrace,
    /// The verifiable report: the finding classes, their verification
    /// status, the read-set and the coverage.
    pub report: InvestigationReport,
}

/// The investigator.
pub struct Investigator {
    retriever: Arc<dyn EvidenceRetriever>,
    /// The model is **optional**: there are deployments without a model, and in them the
    /// investigation is the tools' one.
    model: Option<Arc<dyn ModelProvider>>,
    guard: Arc<std::sync::Mutex<CapabilityGuard>>,
}

impl Investigator {
    /// The investigator with its retriever, its model and its guard.
    pub fn new(
        retriever: Arc<dyn EvidenceRetriever>,
        model: Arc<dyn ModelProvider>,
        guard: CapabilityGuard,
    ) -> Self {
        Self {
            retriever,
            model: Some(model),
            guard: Arc::new(std::sync::Mutex::new(guard)),
        }
    }

    /// The investigator **without a model**: the one that asks nobody.
    ///
    /// Its investigation is the tools' one: the facts they
    /// computed and the uncertainties that are declared. The guard remains
    /// in place: the actions that are proposed wait for the approval of a
    /// person, here too.
    pub fn without_a_model(retriever: Arc<dyn EvidenceRetriever>, guard: CapabilityGuard) -> Self {
        Self {
            retriever,
            model: None,
            guard: Arc::new(std::sync::Mutex::new(guard)),
        }
    }

    /// Investigates a question about a subject.
    ///
    /// Returns the **valid** claims and the trace. If any
    /// claim is not valid, it returns **nothing**: an answer with a
    /// fabricated claim inside is an answer on which nothing can
    /// be relied.
    ///
    /// Without a model there is nobody to ask: the investigation is declared as not
    /// possible, and a half answer is not returned.
    pub async fn investigate(
        &self,
        investigation: &str,
        scope: &RetrievalScope,
        question: &str,
        now: DateTime<Utc>,
    ) -> Result<Investigation, InvestigationError> {
        let model = self.model.as_ref().ok_or_else(|| {
            InvestigationError::NoAnswer("there is no model in this deployment".to_string())
        })?;

        // 1. The retrieval, and its check.
        let context = self.the_context(scope).await?;
        let facts = &context.facts;

        // 2. The question, with the delimited context.
        let request = the_prompt(question, facts);
        let response = model
            .complete(&request)
            .await
            .map_err(InvestigationError::NoAnswer)?;

        // 3. The validation of the structured answer.
        let claims = the_claims(&response.text)?;

        // And for each claim, that the citations really resolve.
        let known: Vec<String> = facts.iter().map(|fact| fact.evidence_id.clone()).collect();
        for claim in &claims {
            check_the_claim(claim, &known, the_allowed_kinds())
                .map_err(InvestigationError::InvalidClaim)?;
        }

        // The three manifest lists: what was read, what was
        // given to the model and what the accepted claims cited. They are
        // three different things and the trace distinguishes them.
        let sent_to_model: Vec<String> =
            facts.iter().map(|fact| fact.evidence_id.clone()).collect();
        let mut cited: Vec<String> = claims
            .iter()
            .flat_map(|claim| claim.citations.clone())
            .collect();
        cited.sort();
        cited.dedup();

        // 4. The report, BEFORE moving the read-set to the trace: the
        // finding classes, with their deterministic verification status,
        // their origin and their limits. The same claims and the same
        // read-set give the same report.
        let report = build_report(&scope.subject, &claims, &context.read_set, context.coverage);

        // 5. The trace, with the coverage: how much of what exists was given
        // to the model. A partial context is declared, never hidden.
        let trace = InvestigationTrace::new(
            investigation,
            response.provider,
            response.model,
            response.model_version,
            PROMPT_VERSION,
            known,
            context.coverage,
            context.read_set,
            sent_to_model,
            cited,
            now,
        );

        Ok(Investigation {
            claims,
            trace,
            report,
        })
    }

    /// The investigation **without a model**: the tools' one.
    ///
    /// It is the same operation, with the same retrieval and the same scope
    /// control, but without the question to the model: what is delivered is
    /// the facts the tools computed and the uncertainties that
    /// are declared. The question is declared **unanswered**, because without a
    /// model nobody answers it, and an answer that pretends to have
    ///answered it is a fabrication.
    pub async fn investigate_without_a_model(
        &self,
        investigation: &str,
        scope: &RetrievalScope,
        question: &str,
        now: DateTime<Utc>,
    ) -> Result<Investigation, InvestigationError> {
        let context = self.the_context(scope).await?;
        let facts = &context.facts;

        // The claims that the tools compute, and the
        // uncertainty that declares the limits of an answer without a
        // model.
        let mut claims = crate::deterministic::the_computed_claims(facts);
        claims.push(crate::deterministic::the_uncertainty_of_a_model_less_answer(question));

        // What the claims cited. Without a model nothing was given to
        // anybody: what was read is the context, and the manifest says so.
        let known: Vec<String> = facts.iter().map(|fact| fact.evidence_id.clone()).collect();
        let mut cited: Vec<String> = claims
            .iter()
            .flat_map(|claim| claim.citations.clone())
            .collect();
        cited.sort();
        cited.dedup();

        let report = build_report(&scope.subject, &claims, &context.read_set, context.coverage);

        let trace = InvestigationTrace::new(
            investigation,
            crate::deterministic::DETERMINISTIC_PROVIDER,
            crate::deterministic::NO_MODEL,
            None,
            crate::deterministic::NO_PROMPT_VERSION,
            known,
            context.coverage,
            context.read_set,
            Vec::new(),
            cited,
            now,
        );

        Ok(Investigation {
            claims,
            trace,
            report,
        })
    }

    /// The retrieval within the scope, and its check.
    ///
    /// The retrieval is **checked** against the scope again, because
    /// a retriever that makes a mistake cannot let anything in. And a scope
    /// that crosses accounts is closed BEFORE asking for anything: an actor of account
    /// A and a subject of account B do not form an investigation, and the
    /// rejection comes before retrieving.
    async fn the_context(
        &self,
        scope: &RetrievalScope,
    ) -> Result<RetrievedContext, InvestigationError> {
        if scope.crosses_accounts() {
            return Err(InvestigationError::NotAuthorized {
                actor: scope.actor.to_string(),
                resource: scope.subject.to_string(),
            });
        }

        let context = match self.retriever.retrieve(scope).await {
            Ok(context) => context,
            Err(RetrievalError::NotAuthorized { actor, resource }) => {
                return Err(InvestigationError::NotAuthorized { actor, resource });
            }
            Err(RetrievalError::Stale {
                source,
                expected,
                found,
            }) => {
                return Err(InvestigationError::Stale(format!(
                    "{source} was at generation {expected} and is at {found}"
                )));
            }
            Err(RetrievalError::Unavailable(reason)) => {
                return Err(InvestigationError::NoAnswer(reason));
            }
        };
        check_the_scope(scope, &context.facts).map_err(InvestigationError::OutOfScope)?;
        Ok(context)
    }

    /// Records an action the investigator wants to take.
    ///
    /// It does not execute it: it leaves it waiting for the approval of a person, and
    /// that is all it can do.
    pub fn propose(&self, action: ProposedAction) -> crate::guard::TracedAction {
        let mut guard = self.guard.lock().expect("la guardia no esta envenenada");
        guard.propose(action).clone()
    }

    /// The guard.
    pub fn guard(&self) -> &Arc<std::sync::Mutex<CapabilityGuard>> {
        &self.guard
    }
}

/// The claim types that are admitted.
///
/// The four of the domain. The restriction is not on the types but on what
/// is done with them: facts are checked against the
/// tools, and inferences are declared as such.
fn the_allowed_kinds() -> &'static [ClaimKind] {
    &[
        ClaimKind::Fact,
        ClaimKind::Inference,
        ClaimKind::Unknown,
        ClaimKind::Recommendation,
    ]
}

/// Reads the model's structured answer.
///
/// If it cannot be read, **nothing** is published.
fn the_claims(text: &str) -> Result<Vec<AIClaim>, InvestigationError> {
    let parsed: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|error| InvestigationError::Unreadable(error.to_string()))?;
    let claims: Vec<AIClaim> = serde_json::from_value(parsed)
        .map_err(|error| InvestigationError::Unreadable(error.to_string()))?;
    if claims.is_empty() {
        return Err(InvestigationError::Unreadable(
            "the answer has no claims".to_string(),
        ));
    }
    Ok(claims)
}

/// The read action, which needs no approval.
pub fn a_read(subject: impl Into<String>, requested_by: impl Into<String>) -> ProposedAction {
    ProposedAction {
        kind: ActionKind::Read,
        description: "read the evidence of the subject".to_string(),
        subject: subject.into(),
        requested_by: requested_by.into(),
        requested_at: Utc::now(),
    }
}
