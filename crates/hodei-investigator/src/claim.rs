//! The investigation's claim: what the model says, with its
//! provenance.
//!
//! An AIClaim is not text: it is a **typed** claim that says what
//! kind it is, **cites** the evidence it comes from and carries the trace of who
//! produced it. And a claim without a resolvable citation is not a claim:
//! it is a fabrication, and it is not published.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What kind of claim it is.
///
/// The distinction between a **fact** and an **inference** is what
/// makes the explanation auditable: the fact was computed by the
/// deterministic tools, and the inference was made by the model. An
/// explanation that does not distinguish them is one on which
/// nothing can be relied upon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimKind {
    /// A fact computed by the deterministic tools.
    Fact,
    /// An inference by the model from the facts.
    Inference,
    /// Something that is not known.
    ///
    /// The model may say "I don't know", and that is the HONEST thing: a
    /// declared uncertainty is an invitation to check, and an
    /// invented certainty is an invitation to check nothing.
    Unknown,
    /// A recommendation.
    Recommendation,
}

/// A claim with its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AIClaim {
    /// What is claimed.
    pub statement: String,
    /// What kind it is.
    pub kind: ClaimKind,
    /// The citations: the identifiers of the evidence it comes from.
    pub citations: Vec<String>,
}

impl AIClaim {
    /// A claim.
    pub fn new(statement: impl Into<String>, kind: ClaimKind, citations: Vec<String>) -> Self {
        Self {
            statement: statement.into(),
            kind,
            citations,
        }
    }
}

/// Why a claim is not valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimError {
    /// It says nothing.
    Empty,
    /// A fact without a citation is a fabrication.
    AFactWithoutCitations,
    /// A citation that does not resolve.
    UnresolvedCitation(String),
    /// A claim that is not of the requested kind.
    NotAllowed(ClaimKind),
}

impl std::fmt::Display for ClaimError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(formatter, "the claim says nothing"),
            Self::AFactWithoutCitations => {
                write!(formatter, "a fact without a citation is a fabrication")
            }
            Self::UnresolvedCitation(citation) => {
                write!(formatter, "the citation {citation} does not resolve")
            }
            Self::NotAllowed(kind) => write!(
                formatter,
                "a claim of kind {kind:?} is not allowed where it is"
            ),
        }
    }
}

/// Checks a claim against the evidence that was retrieved.
///
/// The rules are three and are deliberate:
///
/// - a **fact** needs at least one citation, and the citation has to
///   resolve: a fact without evidence is a fabrication;
/// - an **inference** may not cite, but then it is an opinion and
///   is declared as such;
/// - an **uncertainty** needs no citations: what it says is that it is not
///   known.
pub fn check_the_claim(
    claim: &AIClaim,
    known_evidence: &[String],
    allowed: &[ClaimKind],
) -> Result<(), ClaimError> {
    if claim.statement.trim().is_empty() {
        return Err(ClaimError::Empty);
    }
    if !allowed.contains(&claim.kind) {
        return Err(ClaimError::NotAllowed(claim.kind));
    }
    for citation in &claim.citations {
        if !known_evidence.contains(citation) {
            return Err(ClaimError::UnresolvedCitation(citation.clone()));
        }
    }
    if claim.kind == ClaimKind::Fact && claim.citations.is_empty() {
        return Err(ClaimError::AFactWithoutCitations);
    }
    Ok(())
}

/// The trace of an investigation.
///
/// The trace says **who** produced the answer, with **which** prompt, on **which**
/// context and **when**. Without that, an answer cannot be
/// reproduced or audited, and an explanation that cannot be audited
/// is not an explanation: it is an opinion with formatting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvestigationTrace {
    /// The investigation's identifier.
    pub investigation: String,
    /// Who produced the answer.
    pub provider: String,
    /// The model.
    pub model: String,
    /// The model's version, if any.
    pub model_version: Option<String>,
    /// The prompt template's version.
    pub prompt_version: String,
    /// The identifiers of the evidence it was given.
    pub context_evidence: Vec<String>,
    /// How many words the context had.
    pub context_words: usize,
    /// How much of what exists was handed to the model.
    ///
    /// A partial retrieval is DECLARED in the trace: an
    /// incomplete context cannot pass as an exhaustive answer.
    #[serde(default)]
    pub coverage: crate::scope::Coverage,
    /// The manifest of what was RETRIEVED, with its digests and
    /// generations. It carries no content: identifiers and digests.
    #[serde(default)]
    pub retrieved: crate::read_set::ReadSet,
    /// What was GIVEN to the model: the identifiers of the facts
    /// that went into the prompt.
    #[serde(default)]
    pub sent_to_model: Vec<String>,
    /// What the ACCEPTED claims cited. A fact that reached the
    /// model and was not cited supports nothing.
    #[serde(default)]
    pub cited: Vec<String>,
    /// When it was made.
    pub at: DateTime<Utc>,
}

impl InvestigationTrace {
    /// The trace of an investigation.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        investigation: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
        model_version: Option<String>,
        prompt_version: impl Into<String>,
        context_evidence: Vec<String>,
        coverage: crate::scope::Coverage,
        retrieved: crate::read_set::ReadSet,
        sent_to_model: Vec<String>,
        cited: Vec<String>,
        at: DateTime<Utc>,
    ) -> Self {
        Self {
            investigation: investigation.into(),
            provider: provider.into(),
            model: model.into(),
            model_version,
            prompt_version: prompt_version.into(),
            context_words: context_evidence.len(),
            context_evidence,
            coverage,
            retrieved,
            sent_to_model,
            cited,
            at,
        }
    }
}
