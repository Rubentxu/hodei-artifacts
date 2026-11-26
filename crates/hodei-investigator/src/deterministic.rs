//! The deterministic investigation: the one that does not ask a model.
//!
//! The model is **optional** in the investigator, and when it is absent, the
//! investigation does not degrade into an empty answer: it is the answer of the
//! **tools**. What the tools computed are the facts,
//! with their citation and their seal; and what is not known is **declared** as
//! uncertainty, because invented certainty is an invitation not to
//! check anything.
//!
//! # What this investigation does NOT do
//!
//! - It does **not interpret** the evidence. The text of a third party is data, and
//!   here there is nobody to interpret it: what is delivered is what the
//!   tools computed, and the text of the evidence stays in the
//!   context that was read, not in the claims.
//! - It does **not answer** the question. Without a model, the question is declared
//!   unanswered, and what is delivered are the facts and the uncertainties.
//! - It does **not decide** anything. The report describes; decisions are still
//!   made by the deterministic policy engine.

use hodei_evidence::domain::graph::NodeKind;

use crate::claim::{AIClaim, ClaimKind};
use crate::scope::{FactStatus, ScopedFact};

/// The version of the template of a model-less investigation.
///
/// There is no prompt, and the trace says so: a model-less investigation cannot
/// be compared with one that has a model, and a trace that pretends a prompt
/// that never existed lies.
pub const NO_PROMPT_VERSION: &str = "none";

/// Who produces the claims of a deterministic investigation.
///
/// It is not a model: it is the tools, and the report says so.
pub const DETERMINISTIC_PROVIDER: &str = "deterministic";

/// The name of the "model" of a deterministic investigation.
///
/// There is no model, and the field says so instead of inventing one.
pub const NO_MODEL: &str = "none";

/// The claims that the tools compute.
///
/// A fact is a **relation that the world has**: the statement names it
/// with the two HRNs and the relation word, and the deterministic verifier
/// checks it against the seals of the read-set. A fact that the verifier
/// cannot check is NOT a fact: it is a claim with a citation, and it is
/// declared as such.
///
/// The text of the evidence is NOT published as a claim: it was written by a
/// third party, and here there is nobody to interpret it. It is context, and the
/// manifest of what was read says so.
///
/// And what could not be read is declared, with the reason: an absent
/// evidence, a store that did not answer, a text that could not be decoded
/// and a budget that left something out are four different things, and
/// none is disguised as another.
pub fn the_computed_claims(facts: &[ScopedFact]) -> Vec<AIClaim> {
    let mut claims = Vec::new();
    for fact in facts {
        match fact.status {
            // The CONTEXT read. The text is from a third party: it is what was
            // read, not what is claimed, and it is not published as a claim.
            //
            // The EXCEPTION is the graph relation: a fact of type
            // Artifact is a relation that the world has, and its statement
            // says it with its two HRNs and its relation word.
            FactStatus::Loaded => {
                if fact.kind == NodeKind::Artifact {
                    claims.push(AIClaim::new(
                        fact.text.clone(),
                        ClaimKind::Fact,
                        vec![fact.evidence_id.clone()],
                    ));
                }
            }
            // What could not be read, with the reason.
            FactStatus::Missing => {
                claims.push(the_uncertainty(format!(
                    "the content of the evidence {} is not there",
                    fact.evidence_id
                )));
            }
            FactStatus::Unavailable => {
                claims.push(the_uncertainty(format!(
                    "the content of the evidence {} could not be read: the store did not answer",
                    fact.evidence_id
                )));
            }
            FactStatus::Undecodable => {
                claims.push(the_uncertainty(format!(
                    "the content of the evidence {} is not text",
                    fact.evidence_id
                )));
            }
            FactStatus::OmittedByBudget => {
                claims.push(the_uncertainty(format!(
                    "the {} of the evidence {} was left out by the budget",
                    fact.kind, fact.evidence_id
                )));
            }
        }
    }
    claims
}

/// The uncertainty that declares the limits of a model-less answer.
///
/// Without a model there is nobody to interpret the evidence nor to answer the
/// question, and that is said: what is delivered are the facts that the
/// tools computed, and not an extra explanation.
pub fn the_uncertainty_of_a_model_less_answer(question: &str) -> AIClaim {
    let question = question.trim();
    if question.is_empty() {
        return the_uncertainty(
            "there is no model in this deployment: what follows is what the tools computed, and \
             nothing is inferred from it"
                .to_string(),
        );
    }
    the_uncertainty(format!(
        "there is no model in this deployment, so the question is not answered: what follows is \
         what the tools computed about the subject, and nothing is inferred from it. The \
         question was: {question}"
    ))
}

/// An uncertainty: what is not known, stated as not known.
///
/// An uncertainty cites nothing: it asserts nothing, and so it needs no
/// evidence to support it.
fn the_uncertainty(statement: String) -> AIClaim {
    AIClaim::new(statement, ClaimKind::Unknown, Vec::new())
}
