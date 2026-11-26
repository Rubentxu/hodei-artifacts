//! The contract of the counterfactual investigation.
//!
//! It is not new prose: it is the investigation's `InvestigationReport` and
//! the simulation's `SimulationResult`, joined by
//! labeled statements that say where each thing comes from and by a
//! seal that checks that both halves looked at the same world.

use serde::{Deserialize, Serialize};

use hodei_investigator::read_set::ReadSet;
use hodei_investigator::report::{FindingKind, InvestigationReport, VerificationStatus};
use hodei_simulation::result::{
    BaseStamp, Hypothesis, HypothesisKind, SimulationResult, SourceStamp,
};

/// The version of the counterfactual report contract.
pub const COUNTERFACTUAL_REPORT_VERSION: &str = "1.0.0";

/// The epistemic class of a report statement.
///
/// The classes are not mixed: what is verified is verified by the deterministic
/// verifier, what is hypothetical is so under the declared hypothesis, and the
/// recommendation is stated by whoever asks -- never by the policy engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicLabel {
    /// The deterministic verifier verified it against the current
    /// evidence.
    Verified,
    /// It was observed and the citation resolves, but nobody verified the
    /// statement.
    Observed,
    /// It only holds under the scenario's hypothesis: it is not a fact of the
    /// graph.
    Hypothetical,
    /// An inference by whoever narrates: it is declared as an inference.
    Inference,
    /// What is not known, stated as what is not known.
    Unknown,
    /// A recommendation: nobody executed it and it does not change the trust
    /// state.
    Recommendation,
}

/// A statement of the report, with its class and its references.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    /// The epistemic class of the statement.
    pub label: EpistemicLabel,
    /// What is asserted.
    pub statement: String,
    /// Where it comes from: verifier, model, counter-history or policy
    /// engine.
    pub origin: String,
    /// The references: cited evidence, policy seals and
    /// scenario identifiers.
    pub references: Vec<String>,
}

/// The check that both halves spoke of the same world.
///
/// The generations shared between the investigation manifest
/// and the sources of the simulated base must match: otherwise there is no
/// answer, there are two questions about different worlds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealCoherence {
    /// Whether the seal is coherent.
    pub coherent: bool,
    /// The shared sources with the version in which they match.
    pub shared: Vec<(String, u64)>,
    /// The discrepancies, in text, without subject identifiers.
    pub mismatched: Vec<String>,
}

/// The complete counterfactual report.
///
/// It is versioned and serializable, and decides nothing: it describes the cause
/// with references, the hypothesis as a hypothesis and the difference in
/// decisions as a computed difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterfactualReport {
    /// The contract version.
    pub version: String,
    /// The identifier of the counterfactual investigation.
    pub counterfactual: String,
    /// The question that was answered.
    pub question: String,
    /// The subject that was asked about.
    pub subject: String,
    /// The labeled statements, in canonical order: first the
    /// investigation, then the counter-history.
    pub statements: Vec<Statement>,
    /// The check of the shared seal.
    pub seal: SealCoherence,
    /// The seal of the base on which it was simulated.
    pub base: BaseStamp,
    /// The entire investigation, as its own contract.
    pub investigation: InvestigationReport,
    /// The entire simulation, as a versioned contract.
    pub simulation: SimulationResult,
}

/// The coherence of the seal: the sources that both manifests
/// share, with the version each one declares.
pub fn the_seal_coherence(read_set: &ReadSet, sources: &[SourceStamp]) -> SealCoherence {
    let mut shared = Vec::new();
    let mut mismatched = Vec::new();
    for (source, version) in &read_set.generations {
        match sources.iter().find(|known| &known.source == source) {
            Some(known) if known.version == *version => {
                shared.push((source.clone(), *version));
            }
            Some(known) => mismatched.push(format!(
                "the {source} was read at version {} and simulated at version {}",
                version, known.version
            )),
            None => {}
        }
    }
    SealCoherence {
        coherent: mismatched.is_empty(),
        shared,
        mismatched,
    }
}

/// The statement of an investigation finding, with its label.
fn the_statement_of(finding: &hodei_investigator::report::Finding) -> Statement {
    let (label, origin) = match (finding.kind, finding.status) {
        (FindingKind::VerifiedFact, _) => (EpistemicLabel::Verified, "deterministic-verifier"),
        (FindingKind::EvidenceBackedClaim, VerificationStatus::Contradicted) => {
            (EpistemicLabel::Unknown, "deterministic-verifier")
        }
        (FindingKind::EvidenceBackedClaim, _) => (EpistemicLabel::Observed, "model"),
        (FindingKind::Inference, _) => (EpistemicLabel::Inference, "model"),
        (FindingKind::Uncertainty, _) => (EpistemicLabel::Unknown, "model"),
        (FindingKind::Recommendation, _) => (EpistemicLabel::Recommendation, "model"),
    };
    Statement {
        label,
        statement: finding.statement.clone(),
        origin: origin.to_string(),
        references: finding.citations.clone(),
    }
}

/// The statement of a scenario hypothesis.
fn the_statement_of_hypothesis(hypothesis: &Hypothesis) -> Statement {
    let described = match &hypothesis.kind {
        HypothesisKind::Relation {
            relation,
            from,
            to,
            replaces,
        } => match replaces {
            Some(identity) => format!(
                "the relation {} from {from} to {to} would replace {identity}",
                relation.as_str()
            ),
            None => format!(
                "the relation {} from {from} to {to} would exist",
                relation.as_str()
            ),
        },
        HypothesisKind::Risk {
            subject,
            advisory,
            critical,
            reachable,
        } => format!(
            "the risk {advisory} (critical: {critical}, reachable: {reachable}) would reach {subject}"
        ),
        HypothesisKind::UntrustedWindow { from, to, .. } => {
            format!("the builder would not be trusted between {from} and {to}")
        }
        HypothesisKind::Policy { policy } => {
            format!(
                "the policy {}@{} would be in force",
                policy.id, policy.version
            )
        }
    };
    Statement {
        label: EpistemicLabel::Hypothetical,
        statement: described,
        origin: "counterfactual".to_string(),
        references: std::iter::once(hypothesis.simulation.clone())
            .chain(
                hypothesis
                    .subjects
                    .iter()
                    .map(|subject| subject.to_string()),
            )
            .collect(),
    }
}

/// The statements added by the counter-history: the relations that
/// would appear, those that would disappear and the decisions that change.
fn the_statements_of_the_simulation(simulation: &SimulationResult) -> Vec<Statement> {
    let mut statements: Vec<Statement> = simulation
        .hypotheses
        .iter()
        .map(the_statement_of_hypothesis)
        .collect();

    for relation in &simulation.structural.appeared {
        statements.push(Statement {
            label: EpistemicLabel::Hypothetical,
            statement: format!("the relation {} would appear", relation.identity()),
            origin: "counterfactual".to_string(),
            references: vec![
                simulation.simulation.clone(),
                relation.identity(),
                relation.evidence.clone().unwrap_or_default(),
            ],
        });
    }
    for relation in &simulation.structural.disappeared {
        statements.push(Statement {
            label: EpistemicLabel::Hypothetical,
            statement: format!("the relation {} would disappear", relation.identity()),
            origin: "counterfactual".to_string(),
            references: vec![
                simulation.simulation.clone(),
                relation.identity(),
                relation.evidence.clone().unwrap_or_default(),
            ],
        });
    }
    for change in &simulation.decisions {
        statements.push(Statement {
            label: EpistemicLabel::Hypothetical,
            statement: format!(
                "the decision of {} would change from {:?} to {:?}",
                change.subject, change.before, change.after
            ),
            // The change is computed by the policy engine over the
            // hypothesis: it is the only sentence of the report whose origin is
            // the engine, and therefore no recommendation can come out of it.
            origin: "policy-engine".to_string(),
            references: vec![
                simulation.simulation.clone(),
                change.subject.to_string(),
                format!("policy@{}", change.policy_version_before),
                format!("policy@{}", change.policy_version_after),
            ],
        });
    }
    statements
}

/// The report from the two halves and their seal.
///
/// Construction is **pure**: it does not read, write or decide.
pub fn build_report(
    counterfactual: &str,
    question: &str,
    investigation: &InvestigationReport,
    simulation: &SimulationResult,
    seal: SealCoherence,
) -> CounterfactualReport {
    let mut statements: Vec<Statement> = investigation
        .findings
        .iter()
        .map(the_statement_of)
        .collect();
    statements.extend(the_statements_of_the_simulation(simulation));

    CounterfactualReport {
        version: COUNTERFACTUAL_REPORT_VERSION.to_string(),
        counterfactual: counterfactual.to_string(),
        question: question.to_string(),
        subject: investigation.subject.clone(),
        statements,
        seal,
        base: simulation.base.clone(),
        investigation: investigation.clone(),
        simulation: simulation.clone(),
    }
}
