//! El contrato de la investigacion contrafactual.
//!
//! No es una prosa nueva: es el `InvestigationReport` de la
//! investigacion y el `SimulationResult` de la simulacion, unidos por
//! enunciados etiquetados que dicen de donde sale cada cosa y por un
//! sello que comprueba que las dos mitades miraron el mismo mundo.

use serde::{Deserialize, Serialize};

use hodei_investigator::read_set::ReadSet;
use hodei_investigator::report::{FindingKind, InvestigationReport, VerificationStatus};
use hodei_simulation::result::{
    BaseStamp, Hypothesis, HypothesisKind, SimulationResult, SourceStamp,
};

/// La version del contrato del informe contrafactual.
pub const COUNTERFACTUAL_REPORT_VERSION: &str = "1.0.0";

/// La clase epistemica de un enunciado del informe.
///
/// Las clases no se mezclan: lo verificado lo es por el verificador
/// determinista, lo hipotetico lo es bajo la hipotesis declarada, y la
/// recomendacion la dice quien pregunta -- nunca el motor de politicas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpistemicLabel {
    /// El verificador determinista lo verifico contra la evidencia
    /// vigente.
    Verified,
    /// Se observo y la cita resuelve, pero nadie verifico el
    /// enunciado.
    Observed,
    /// Solo vale bajo la hipotesis del escenario: no es un hecho del
    /// grafo.
    Hypothetical,
    /// Una inferencia de quien narra: se declara como inferencia.
    Inference,
    /// Lo que no se sabe, dicho como lo que no se sabe.
    Unknown,
    /// Una recomendacion: nadie la ejecuto y no cambia el estado de
    /// confianza.
    Recommendation,
}

/// Un enunciado del informe, con su clase y sus referencias.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    /// La clase epistemica del enunciado.
    pub label: EpistemicLabel,
    /// Que se afirma.
    pub statement: String,
    /// De donde sale: verificador, modelo, contrahistoria o motor de
    /// politicas.
    pub origin: String,
    /// Las referencias: evidencias citadas, sellos de politica e
    /// identificadores del escenario.
    pub references: Vec<String>,
}

/// La comprobacion de que las dos mitades hablaron del mismo mundo.
///
/// Las generaciones compartidas entre el manifiesto de la investigacion
/// y las fuentes de la base simulada tienen que coincidir: si no, no
/// hay respuesta, hay dos preguntas sobre mundos distintos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealCoherence {
    /// Si el sello es coherente.
    pub coherent: bool,
    /// Las fuentes compartidas con la version en la que coinciden.
    pub shared: Vec<(String, u64)>,
    /// Las discrepancias, en texto, sin identificadores de sujetos.
    pub mismatched: Vec<String>,
}

/// El informe contrafactual completo.
///
/// Es versionado y serializable, y no decide nada: describe la causa
/// con referencias, la hipotesis como hipotesis y la diferencia de
/// decisiones como diferencia calculada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CounterfactualReport {
    /// La version del contrato.
    pub version: String,
    /// El identificador de la investigacion contrafactual.
    pub counterfactual: String,
    /// La pregunta que se respondo.
    pub question: String,
    /// El sujeto sobre el que se pregunto.
    pub subject: String,
    /// Los enunciados etiquetados, en orden canonico: primero la
    /// investigacion, luego la contrahistoria.
    pub statements: Vec<Statement>,
    /// La comprobacion del sello compartido.
    pub seal: SealCoherence,
    /// El sello de la base sobre la que se simulo.
    pub base: BaseStamp,
    /// La investigacion entera, como contrato propio.
    pub investigation: InvestigationReport,
    /// La simulacion entera, como contrato versionado.
    pub simulation: SimulationResult,
}

/// La coherencia del sello: las fuentes que los dos manifiestos
/// comparten, con la version que cada uno declara.
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

/// El enunciado de un finding de la investigacion, con su etiqueta.
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

/// El enunciado de una hipotesis del escenario.
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

/// Los enunciados que añade la contrahistoria: las relaciones que
/// aparecerian, las que desaparecerian y las decisiones que cambian.
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
            // El cambio lo calcula el motor de politicas sobre la
            // hipotesis: es la unica frase del informe cuyo origen es
            // el motor, y por eso ninguna recomendacion puede salir de el.
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

/// El informe a partir de las dos mitades y su sello.
///
/// La construccion es **pura**: no lee, no escribe y no decide.
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
