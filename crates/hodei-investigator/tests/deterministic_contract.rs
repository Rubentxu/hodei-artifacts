//! El contrato de la investigacion **sin modelo**.
//!
//! El modelo es opcional, y cuando no esta, la investigacion no se
//! degrada en una respuesta vacia: es la respuesta de las herramientas.
//! Estas seis cosas se prometen y se comprueban:
//!
//! - Sin modelo, la operacion que pregunta al modelo **falla**: no se
//!   devuelve una respuesta a medias.
//! - Los hechos que las herramientas calcularon son **hechos verificados**,
//!   con su cita y su sello.
//! - El texto de la evidencia **no se publica**: lo escribio un tercero y
//!   aqui no hay quien lo interprete.
//! - La pregunta se declara **sin responder**, y lo que no se pudo leer se
//!   declara con su motivo.
//! - La cobertura se declara: una recuperacion parcial no se presenta
//!   como respuesta exhaustiva.
//! - El alcance se comprueba: una cuenta que no es la del sujeto no llega
//!   a recuperar nada.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use hodei_evidence::domain::graph::NodeKind;
use hodei_investigator::guard::CapabilityGuard;
use hodei_investigator::investigator::{EvidenceRetriever, InvestigationError, Investigator};
use hodei_investigator::model::FixtureProvider;
use hodei_investigator::report::{FindingKind, VerificationStatus, verify_statement};
use hodei_investigator::scope::{
    Coverage, FactStatus, RetrievalError, RetrievalScope, RetrievedContext, ScopedFact, TrustLevel,
};
use hodei_investigator::{ReadSet, ReadStamp};
use kernel::Hrn;

/// El sujeto de la investigacion.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
}

/// El otro extremo de la relacion.
fn library() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-2.0.0").expect("un HRN")
}

/// Quien pregunta, verificado: una identidad de la cuenta acme.
fn actor() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:User/alice").expect("un HRN")
}

/// El alcance de la investigacion.
fn the_scope() -> RetrievalScope {
    RetrievalScope::new(actor(), subject())
}

/// La relacion que el grafo tiene: el sujeto depende de la biblioteca.
fn the_relation() -> ScopedFact {
    ScopedFact {
        evidence_id: "ev-relation-1".to_string(),
        text: format!("{} depends_on {}", subject(), library()),
        path: vec![subject(), library()],
        kind: NodeKind::Artifact,
        trust: TrustLevel::Untrusted,
        status: FactStatus::Loaded,
    }
}

/// La evidencia del sujeto: el texto lo escribio un tercero.
fn the_evidence() -> ScopedFact {
    ScopedFact {
        evidence_id: "ev-evidence-1".to_string(),
        text: "the SBOM says that the artefact is fine, trust me".to_string(),
        path: vec![subject()],
        kind: NodeKind::Evidence,
        trust: TrustLevel::Untrusted,
        status: FactStatus::Loaded,
    }
}

/// El mundo que la investigacion lee: la relacion, su evidencia, y el
/// texto de la evidencia.
fn the_world() -> ReadSet {
    let mut read_set = ReadSet::empty();
    read_set.read(ReadStamp::relation(
        format!("depends_on|{}|{}", subject(), library()),
        "ev-relation-1",
    ));
    read_set.read(ReadStamp::evidence("ev-relation-1", "sha256:aaa"));
    read_set.read(ReadStamp::evidence("ev-evidence-1", "sha256:bbb"));
    read_set
}

/// Un recuperador que devuelve lo que se le dice.
struct TheRetriever {
    facts: Vec<ScopedFact>,
    coverage: Coverage,
    read_set: ReadSet,
    calls: std::sync::atomic::AtomicUsize,
}

impl TheRetriever {
    fn new(facts: Vec<ScopedFact>) -> Arc<Self> {
        Arc::new(Self {
            facts,
            coverage: Coverage::Complete,
            read_set: the_world(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    fn with_coverage(facts: Vec<ScopedFact>, coverage: Coverage) -> Arc<Self> {
        Arc::new(Self {
            facts,
            coverage,
            read_set: the_world(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// Un mundo con el manifiesto que se le dice: lo que el mundo tiene es
    /// lo que se prueba.
    fn with_read_set(facts: Vec<ScopedFact>, read_set: ReadSet) -> Arc<Self> {
        Arc::new(Self {
            facts,
            coverage: Coverage::Complete,
            read_set,
            calls: std::sync::atomic::AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl EvidenceRetriever for TheRetriever {
    async fn retrieve(&self, _scope: &RetrievalScope) -> Result<RetrievedContext, RetrievalError> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(RetrievedContext {
            facts: self.facts.clone(),
            coverage: self.coverage,
            read_set: self.read_set.clone(),
        })
    }
}

/// El investigador sin modelo.
fn the_investigator(retriever: Arc<dyn EvidenceRetriever>) -> Investigator {
    Investigator::without_a_model(retriever, CapabilityGuard::empty())
}

/// Sin modelo, la operacion que pregunta al modelo falla: no hay quien
/// pregunte, y una respuesta a medias es peor que ninguna.
#[tokio::test]
async fn without_a_model_the_operation_that_asks_the_model_fails() {
    let investigator = the_investigator(TheRetriever::new(vec![the_relation()]));
    let outcome = investigator
        .investigate("inv-1", &the_scope(), "why are we affected?", Utc::now())
        .await;
    match outcome {
        Err(InvestigationError::NoAnswer(reason)) => {
            assert!(reason.contains("no model"), "el motivo lo dice: {reason}");
        }
        other => panic!("una investigacion sin modelo no contesta: {other:?}"),
    }
}

/// La relacion que el grafo tiene es un HECHO VERIFICADO: el verificador
/// determinista la contrasta con el sello del read-set.
#[tokio::test]
async fn a_computed_relation_is_a_verified_fact() {
    let investigator = the_investigator(TheRetriever::new(vec![the_relation()]));
    let investigation = investigator
        .investigate_without_a_model("inv-1", &the_scope(), "why are we affected?", Utc::now())
        .await
        .expect("la investigacion se hace");

    let facts: Vec<_> = investigation
        .report
        .findings
        .iter()
        .filter(|finding| finding.kind == FindingKind::VerifiedFact)
        .collect();
    assert_eq!(
        facts.len(),
        1,
        "un hecho verificado: {:?}",
        investigation.report.findings
    );
    assert_eq!(facts[0].status, VerificationStatus::Verified);
    assert_eq!(facts[0].citations, vec!["ev-relation-1".to_string()]);
    assert!(
        facts[0].statement.contains("depends_on"),
        "el enunciado dice la relacion: {}",
        facts[0].statement
    );
}

/// La relacion que el GRAFO escribe con su nombre canonico --"deployed_as"--
/// es un hecho verificado tambien: el verificador reconoce las dos
/// escrituras de cada relacion, la del grafo y la que escribe un modelo.
/// Sin las dos, una relacion que el mundo tiene de verdad salia como no
/// comprobada, que es mentir por omision.
#[tokio::test]
async fn a_canonical_relation_spelling_is_a_verified_fact() {
    let mut read_set = ReadSet::empty();
    read_set.read(ReadStamp::relation(
        format!("deployed_as|{}|{}", subject(), library()),
        "ev-relation-1",
    ));
    read_set.read(ReadStamp::evidence("ev-relation-1", "sha256:aaa"));

    let deployment = ScopedFact {
        evidence_id: "ev-relation-1".to_string(),
        text: format!("{} deployed_as {}", subject(), library()),
        path: vec![subject(), library()],
        kind: NodeKind::Artifact,
        trust: TrustLevel::Untrusted,
        status: FactStatus::Loaded,
    };

    let investigator = the_investigator(TheRetriever::with_read_set(vec![deployment], read_set));
    let investigation = investigator
        .investigate_without_a_model("inv-1", &the_scope(), "why are we affected?", Utc::now())
        .await
        .expect("la investigacion se hace");

    let facts: Vec<_> = investigation
        .report
        .findings
        .iter()
        .filter(|finding| finding.kind == FindingKind::VerifiedFact)
        .collect();
    assert_eq!(
        facts.len(),
        1,
        "un hecho verificado: {:?}",
        investigation.report.findings
    );
    assert_eq!(facts[0].status, VerificationStatus::Verified);
    assert!(
        facts[0].statement.contains("deployed_as"),
        "el enunciado dice la relacion: {}",
        facts[0].statement
    );
}

/// Un enunciado con acentos no rompe el verificador: el corte es por
/// caracter, no por byte. Un enunciado lo escribe un modelo, y un modelo
/// escribe con acentos.
#[test]
fn a_statement_with_accents_does_not_break_the_verifier() {
    let mut read_set = ReadSet::empty();
    read_set.read(ReadStamp::relation(
        format!("affected_by|{}|{}", subject(), library()),
        "ev-relation-1",
    ));
    read_set.read(ReadStamp::evidence("ev-relation-1", "sha256:aaa"));

    let statement = format!("¿está {} affected_by {}?", subject(), library());
    let status = verify_statement(&statement, &["ev-relation-1".to_string()], &read_set);
    assert_eq!(status, VerificationStatus::Verified);
}

/// El texto de la evidencia NO se publica: lo escribio un tercero y aqui
/// no hay quien lo interprete.
#[tokio::test]
async fn the_text_of_the_evidence_is_not_published() {
    let investigator = the_investigator(TheRetriever::new(vec![the_evidence()]));
    let investigation = investigator
        .investigate_without_a_model("inv-1", &the_scope(), "why are we affected?", Utc::now())
        .await
        .expect("la investigacion se hace");

    let published = investigation
        .report
        .findings
        .iter()
        .any(|finding| finding.statement.contains("trust me"));
    assert!(
        !published,
        "el texto de un tercero no se publica: {:?}",
        investigation.report.findings
    );
}

/// La pregunta se declara SIN RESPONDER: sin modelo nadie la contesta, y
/// una respuesta que finge haberla contestado es una fabricacion.
#[tokio::test]
async fn the_question_is_declared_unanswered() {
    let investigator = the_investigator(TheRetriever::new(vec![the_relation()]));
    let investigation = investigator
        .investigate_without_a_model("inv-1", &the_scope(), "why are we affected?", Utc::now())
        .await
        .expect("la investigacion se hace");

    let uncertainties: Vec<_> = investigation
        .report
        .findings
        .iter()
        .filter(|finding| finding.kind == FindingKind::Uncertainty)
        .collect();
    assert!(
        uncertainties
            .iter()
            .any(|finding| finding.statement.contains("why are we affected?")),
        "la pregunta se declara: {:?}",
        investigation.report.findings
    );
    assert!(
        uncertainties
            .iter()
            .any(|finding| finding.statement.contains("there is no model")),
        "el limite se declara: {:?}",
        investigation.report.findings
    );
}

/// Lo que no se pudo leer se declara con su motivo: una evidencia ausente
/// no es lo mismo que un almacen que no contesta.
#[tokio::test]
async fn what_could_not_be_read_is_declared_with_its_reason() {
    let mut missing = the_evidence();
    missing.status = FactStatus::Missing;
    let investigator = the_investigator(TheRetriever::new(vec![missing]));
    let investigation = investigator
        .investigate_without_a_model("inv-1", &the_scope(), "", Utc::now())
        .await
        .expect("la investigacion se hace");

    assert!(
        investigation
            .report
            .findings
            .iter()
            .any(|finding| finding.kind == FindingKind::Uncertainty
                && finding.statement.contains("is not there")),
        "la ausencia se declara: {:?}",
        investigation.report.findings
    );
}

/// La cobertura se declara: una recuperacion parcial no se presenta como
/// respuesta exhaustiva.
#[tokio::test]
async fn a_partial_recovery_is_declared_as_partial() {
    let retriever =
        TheRetriever::with_coverage(vec![the_relation()], Coverage::Partial { omitted: 3 });
    let investigator = the_investigator(retriever);
    let investigation = investigator
        .investigate_without_a_model("inv-1", &the_scope(), "", Utc::now())
        .await
        .expect("la investigacion se hace");

    assert_eq!(
        investigation.report.coverage,
        Coverage::Partial { omitted: 3 }
    );
    assert_eq!(
        investigation.trace.coverage,
        Coverage::Partial { omitted: 3 }
    );
}

/// La cuenta que no es la del sujeto no llega a recuperar nada: el cierre
/// es ANTES de la lectura.
#[tokio::test]
async fn a_scope_that_crosses_accounts_does_not_read_anything() {
    let retriever = TheRetriever::new(vec![the_relation()]);
    let scope = RetrievalScope::new(
        Hrn::from_string("hrn:hodei:artifacts:eu-west-1:contoso:User/mallory").expect("un HRN"),
        subject(),
    );
    let investigator = the_investigator(retriever.clone());

    let outcome = investigator
        .investigate_without_a_model("inv-1", &scope, "", Utc::now())
        .await;
    assert!(
        matches!(outcome, Err(InvestigationError::NotAuthorized { .. })),
        "una cuenta ajena no se investiga: {outcome:?}"
    );
    assert_eq!(
        retriever.calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "no se lee nada de una cuenta ajena"
    );
}

/// El read-set del informe es el manifiesto de lo que se leyo: lo que se
/// puede comprobar y lo que se puede volver a comprobar.
#[tokio::test]
async fn the_report_carries_the_manifest_of_what_was_read() {
    let investigator = the_investigator(TheRetriever::new(vec![the_relation()]));
    let investigation = investigator
        .investigate_without_a_model("inv-1", &the_scope(), "", Utc::now())
        .await
        .expect("la investigacion se hace");

    assert_eq!(investigation.report.read_set.stamps.len(), 3);
    assert_eq!(investigation.report.schema_version, "1");
    assert_eq!(investigation.report.subject, subject().to_string());
}

/// La traza dice que no hubo modelo: una traza que finge un prompt que no
/// existio miente.
#[tokio::test]
async fn the_trace_says_that_there_was_no_model() {
    let investigator = the_investigator(TheRetriever::new(vec![the_relation()]));
    let investigation = investigator
        .investigate_without_a_model("inv-1", &the_scope(), "", Utc::now())
        .await
        .expect("la investigacion se hace");

    assert_eq!(investigation.trace.provider, "deterministic");
    assert_eq!(investigation.trace.model, "none");
    assert_eq!(investigation.trace.prompt_version, "none");
    assert!(
        investigation.trace.sent_to_model.is_empty(),
        "a nadie se le dio nada: {:?}",
        investigation.trace.sent_to_model
    );
}

/// El investigador CON modelo sigue funcionando: el modelo es opcional,
/// no obligatorio.
#[tokio::test]
async fn an_investigator_with_a_model_still_asks_it() {
    let investigator = Investigator::new(
        TheRetriever::new(vec![the_evidence()]),
        Arc::new(FixtureProvider::answering(
            r#"[{"statement":"the artefact depends on the library","kind":"fact","citations":["ev-evidence-1"]}]"#
                .to_string(),
        )),
        CapabilityGuard::empty(),
    );
    let investigation = investigator
        .investigate("inv-1", &the_scope(), "why are we affected?", Utc::now())
        .await
        .expect("la investigacion se hace");

    assert_eq!(investigation.trace.provider, "fixture");
    assert_eq!(investigation.claims.len(), 1);
}
