//! The contract of the **model-less** investigation.
//!
//! The model is optional, and when it is absent, the investigation does not
//! degrade into an empty answer: it is the answer of the tools.
//! These six things are promised and checked:
//!
//! - Without a model, the operation that asks the model **fails**: no
//!   half answer is returned.
//! - The facts that the tools computed are **verified facts**,
//!   with their citation and their seal.
//! - The text of the evidence is **not published**: a third party wrote it and
//!   there is nobody here to interpret it.
//! - The question is declared **unanswered**, and what could not be read is
//!   declared with its reason.
//! - Coverage is declared: a partial retrieval is not presented
//!   as an exhaustive answer.
//! - Scope is checked: an account that is not the subject's does not get
//!   to retrieve anything.

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

/// The subject of the investigation.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
}

/// The other end of the relation.
fn library() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-2.0.0").expect("un HRN")
}

/// Who asks, verified: an identity of the acme account.
fn actor() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:User/alice").expect("un HRN")
}

/// The scope of the investigation.
fn the_scope() -> RetrievalScope {
    RetrievalScope::new(actor(), subject())
}

/// The relation that the graph has: the subject depends on the library.
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

/// The evidence of the subject: the text was written by a third party.
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

/// The world that the investigation reads: the relation, its evidence, and the
/// text of the evidence.
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

/// A retriever that returns what it is told.
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

    /// A world with the manifest it is told: what the world has is
    /// what is tested.
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

/// The model-less investigator.
fn the_investigator(retriever: Arc<dyn EvidenceRetriever>) -> Investigator {
    Investigator::without_a_model(retriever, CapabilityGuard::empty())
}

/// Without a model, the operation that asks the model fails: there is nobody
/// to ask, and a half answer is worse than none.
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

/// The relation that the graph has is a VERIFIED FACT: the deterministic
/// verifier checks it against the seal of the read-set.
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

/// The relation that the GRAPH writes with its canonical name --"deployed_as"--
/// is a verified fact too: the verifier recognizes both
/// spellings of each relation, the graph's and the one a model writes.
/// Without both, a relation that the world truly has came out as
/// unchecked, which is lying by omission.
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

/// A statement with accents does not break the verifier: the cut is by
/// character, not by byte. A statement is written by a model, and a model
/// writes with accents.
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

/// The text of the evidence is NOT published: a third party wrote it and
/// there is nobody here to interpret it.
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

/// The question is declared UNANSWERED: without a model nobody answers it, and
/// an answer that pretends to have answered it is a fabrication.
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

/// What could not be read is declared with its reason: an absent evidence
/// is not the same as a store that does not answer.
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

/// Coverage is declared: a partial retrieval is not presented as an
/// exhaustive answer.
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

/// The account that is not the subject's does not get to retrieve anything: the
/// closing is BEFORE the read.
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

/// The read-set of the report is the manifest of what was read: what can be
/// checked and what can be checked again.
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

/// The trace says there was no model: a trace that pretends a prompt that did not
/// exist lies.
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

/// The investigator WITH a model still works: the model is optional,
/// not mandatory.
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
