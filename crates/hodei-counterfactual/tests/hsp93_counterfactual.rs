//! The contract of the counterfactual investigation (HSP-93).
//!
//! Four things are promised and checked here:
//!
//! - A vulnerable dependency shows its original cause, the substitution
//!   hypothesis and the computed difference of decisions, with
//!   references.
//! - The answer distinguishes what is verified, observed, hypothetical,
//!   inferred and unknown, with a coherent seal, and no
//!   recommendation is attributed to the policy engine.
//! - A hypothesis from another account, expired or inconsistent is rejected
//!   without leaking data and without spending reads.
//! - Nothing is written: the graph and the declared position of the ledger
//!   remain the same before and after.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_counterfactual::{
    COUNTERFACTUAL_REPORT_VERSION, CounterfactualError, CounterfactualReport,
    CounterfactualService, EpistemicLabel, RejectionReason,
};
use hodei_evidence::domain::graph::RelationType;
use hodei_investigator::guard::CapabilityGuard;
use hodei_investigator::investigator::{EvidenceRetriever, Investigator};
use hodei_investigator::model::FixtureProvider;
use hodei_investigator::read_set::{ReadSet, ReadStamp};
use hodei_investigator::scope::{
    Coverage, RetrievalError, RetrievalScope, RetrievedContext, ScopedFact,
};
use hodei_simulation::hypothetical::{Hypothetical, HypotheticalClaim, RelationFact};
use hodei_simulation::memory::{InMemoryDecisions, InMemoryGraph};
use hodei_simulation::service::SimulationService;
use hodei_simulation::{Checkpoint, SimulationFork, SimulationTag};
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use kernel::Hrn;

/// The instant of the tests: a single one, for both halves.
fn the_now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
        .expect("un instante fijo")
        .with_timezone(&Utc)
}

/// The subject: the blocked release.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0").expect("un HRN")
}

/// The vulnerable component it depends on.
fn lib_v1() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v1").expect("un HRN")
}

/// The component of the hypothetical substitution.
fn lib_v2() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Component/lib-v2").expect("un HRN")
}

/// The vulnerability declared by the evidence.
fn vulnerability() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Vulnerability/GHSA-hsp93-0001")
        .expect("un HRN")
}

/// Whoever asks, verified.
fn actor() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:User/alice").expect("un HRN")
}

/// The scope of the investigation.
fn the_scope() -> RetrievalScope {
    RetrievalScope::new(actor(), subject())
}

/// The policy of now: the one that forbids nothing.
fn the_permissive_policy() -> TrustPolicy {
    TrustPolicy::new("production", "1.0.0", Vec::new())
}

/// The hypothetical policy: the one that forbids the established risk.
fn the_strict_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "2.0.0",
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    )
}

/// The observed relation: the subject depends on lib-v1.
fn the_dependency() -> RelationFact {
    RelationFact::new(RelationType::DependsOn, subject(), lib_v1()).with_evidence("evd-1")
}

/// The observed relation: lib-v1 is affected by the vulnerability.
fn the_advisory() -> RelationFact {
    RelationFact::new(RelationType::AffectedBy, lib_v1(), vulnerability()).with_evidence("evd-2")
}

/// The world: two relations and generation 7.
fn the_world() -> Arc<InMemoryGraph> {
    let graph = InMemoryGraph::new("acme");
    graph.set_the_world(vec![the_dependency(), the_advisory()], 7);
    graph
}

/// The simulation service over the world.
fn the_simulation(graph: Arc<InMemoryGraph>) -> Arc<SimulationService> {
    Arc::new(SimulationService::new(graph, Arc::new(InMemoryDecisions)))
}

/// The retriever of the known world, with its manifest and its call counter:
/// a rejection must not spend reads.
struct TheRetriever {
    pub calls: AtomicUsize,
    /// The version of the graph that the manifest declares to have read.
    pub read_at: u64,
}

#[async_trait]
impl EvidenceRetriever for TheRetriever {
    async fn retrieve(&self, _scope: &RetrievalScope) -> Result<RetrievedContext, RetrievalError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut read_set = ReadSet::empty();
        read_set.generation("graph", self.read_at);
        read_set.read(ReadStamp::evidence("evd-1", "sha256:aaa"));
        read_set.read(ReadStamp::evidence("evd-2", "sha256:bbb"));
        read_set.read(ReadStamp::relation(the_dependency().identity(), "evd-1"));
        read_set.read(ReadStamp::relation(the_advisory().identity(), "evd-2"));
        Ok(RetrievedContext {
            facts: vec![
                ScopedFact::untrusted(
                    "evd-1",
                    "the inventory says app-1.0.0 depends on lib-v1",
                    vec![subject()],
                ),
                ScopedFact::untrusted(
                    "evd-2",
                    "the advisory GHSA-hsp93-0001 reaches lib-v1",
                    vec![subject()],
                ),
            ],
            coverage: Coverage::Complete,
            read_set,
        })
    }
}

/// The model's answer: cause, substitution, recommendation,
/// uncertainty and an observed claim without a verifiable relation.
fn the_answer() -> String {
    serde_json::json!([
        {
            "statement": format!("{} depends_on {}", subject(), lib_v1()),
            "kind": "fact",
            "citations": ["evd-1"]
        },
        {
            "statement": format!("{} affected_by {}", lib_v1(), vulnerability()),
            "kind": "fact",
            "citations": ["evd-2"]
        },
        {
            "statement": "the advisory is published by the upstream vendor",
            "kind": "fact",
            "citations": ["evd-2"]
        },
        {
            "statement": "swapping lib-v1 for lib-v2 would clear the inventory relation",
            "kind": "inference",
            "citations": ["evd-1"]
        },
        {
            "statement": "the release could proceed once lib-v2 is validated",
            "kind": "recommendation",
            "citations": ["evd-1"]
        },
        {
            "statement": "whether the builder was patched is not recorded",
            "kind": "unknown",
            "citations": []
        }
    ])
    .to_string()
}

/// The investigator of the known world.
fn the_investigator(read_at: u64) -> (Arc<Investigator>, Arc<TheRetriever>) {
    let retriever = Arc::new(TheRetriever {
        calls: AtomicUsize::new(0),
        read_at,
    });
    let investigator = Arc::new(Investigator::new(
        retriever.clone(),
        Arc::new(FixtureProvider::answering(the_answer())),
        CapabilityGuard::empty(),
    ));
    (investigator, retriever)
}

/// The counter-history fork: the dependency is substituted and the
/// strict policy comes into force with the established risk.
fn the_fork() -> SimulationFork {
    let tag = SimulationTag::new("sim-93", the_now());
    SimulationFork::new(
        "sim-93",
        Checkpoint::new("checkpoint-93", "acme", the_now(), Some(42), 7),
        the_permissive_policy(),
        the_now(),
    )
    .with(Hypothetical {
        tag: tag.clone(),
        claim: HypotheticalClaim::Relation {
            relation: RelationType::DependsOn,
            from: subject(),
            to: lib_v2(),
            replaces: Some(the_dependency()),
        },
    })
    .with(Hypothetical {
        tag,
        claim: HypotheticalClaim::Risk {
            subject: subject(),
            advisory: "GHSA-hsp93-0001".to_string(),
            critical: true,
            reachable: true,
        },
    })
    .with_policy(the_strict_policy())
}

/// The counterfactual service of the standard world.
async fn the_answer_of_the_standard_world() -> (CounterfactualReport, Arc<InMemoryGraph>) {
    let graph = the_world();
    let (investigator, _retriever) = the_investigator(7);
    let service = CounterfactualService::new(investigator, the_simulation(graph.clone()));
    let report = service
        .answer(
            "cf-1",
            &the_scope(),
            "why is the release blocked?",
            &the_fork(),
            the_now(),
        )
        .await
        .expect("la contrahistoria responde");
    (report, graph)
}

fn statements_with(
    report: &CounterfactualReport,
    label: EpistemicLabel,
) -> Vec<&hodei_counterfactual::Statement> {
    report
        .statements
        .iter()
        .filter(|statement| statement.label == label)
        .collect()
}

// ───────────────────────────── AC1 ─────────────────────────────

/// The answer shows the cause, the substitution hypothesis and the
/// difference of decisions, all with references.
#[tokio::test]
async fn the_report_shows_the_cause_the_hypothesis_and_the_decision_difference() {
    let (report, _graph) = the_answer_of_the_standard_world().await;

    // The original cause: the facts that the verifier verified,
    // with their evidence citations.
    let causes = statements_with(&report, EpistemicLabel::Verified);
    assert_eq!(causes.len(), 2, "la dependencia y el advisory: {causes:?}");
    assert!(
        causes
            .iter()
            .any(|cause| cause.references.contains(&"evd-1".to_string()))
    );
    assert!(
        causes
            .iter()
            .any(|cause| cause.references.contains(&"evd-2".to_string()))
    );

    // The substitution hypothesis, declared as a hypothesis.
    let replaced = report
        .statements
        .iter()
        .find(|statement| statement.statement.contains("would replace"))
        .expect("la sustitucion se declara");
    assert_eq!(replaced.label, EpistemicLabel::Hypothetical);
    assert!(replaced.references.contains(&"sim-93".to_string()));

    // The computed difference of decisions, with the versions of the
    // policy that produced it.
    let changed = report
        .statements
        .iter()
        .find(|statement| statement.origin == "policy-engine")
        .expect("la diferencia de decisiones la calcula el motor");
    assert!(
        changed.statement.contains("Allow") && changed.statement.contains("Deny"),
        "de permitir a denegar bajo la hipotesis: {}",
        changed.statement
    );
    assert!(changed.references.contains(&"policy@1.0.0".to_string()));
    assert!(changed.references.contains(&"policy@2.0.0".to_string()));
}

// ───────────────────────────── AC2 ─────────────────────────────

/// The answer distinguishes the epistemic classes and the seal is
/// coherent.
#[tokio::test]
async fn the_answer_labels_every_epistemic_class_with_a_coherent_seal() {
    let (report, _graph) = the_answer_of_the_standard_world().await;

    for label in [
        EpistemicLabel::Verified,
        EpistemicLabel::Observed,
        EpistemicLabel::Hypothetical,
        EpistemicLabel::Inference,
        EpistemicLabel::Unknown,
    ] {
        assert!(
            !statements_with(&report, label).is_empty(),
            "la clase {label:?} tiene su enunciado"
        );
    }

    // The shared seal: the same source, the same version in the
    // two halves.
    assert!(
        report.seal.coherent,
        "los manifiestos coinciden: {:?}",
        report.seal
    );
    assert!(report.seal.shared.contains(&("graph".to_string(), 7)));

    // And the contract travels with both halves intact and its version.
    assert_eq!(report.version, COUNTERFACTUAL_REPORT_VERSION);
    assert_eq!(report.simulation.version, "1.0.0");
    assert_eq!(report.investigation.schema_version, "1");
}

/// A recommendation is not attributed to the policy engine.
#[tokio::test]
async fn a_recommendation_is_not_attributed_to_the_policy_engine() {
    let (report, _graph) = the_answer_of_the_standard_world().await;

    let recommendations = statements_with(&report, EpistemicLabel::Recommendation);
    assert!(!recommendations.is_empty(), "la recomendacion se declara");
    for statement in recommendations {
        assert_eq!(
            statement.origin, "model",
            "quien recomienda es quien narra, no las reglas"
        );
    }
    for statement in &report.statements {
        if statement.origin == "policy-engine" {
            assert_eq!(
                statement.label,
                EpistemicLabel::Hypothetical,
                "el motor solo aparece con la hipotesis: {:?}",
                statement
            );
        }
    }
}

// ───────────────────────────── AC3 ─────────────────────────────

/// A hypothesis from another account is rejected without leaking data and without
/// spending reads.
#[tokio::test]
async fn a_hypothesis_of_another_account_is_rejected_without_leaking() {
    let graph = the_world();
    let (investigator, retriever) = the_investigator(7);
    let service = CounterfactualService::new(investigator, the_simulation(graph));

    let foreign = SimulationFork::new(
        "sim-93",
        Checkpoint::new("checkpoint-93", "contoso", the_now(), Some(42), 7),
        the_permissive_policy(),
        the_now(),
    );
    let error = service
        .answer(
            "cf-2",
            &the_scope(),
            "why is the release blocked?",
            &foreign,
            the_now(),
        )
        .await
        .expect_err("la cuenta no es esta");

    match error {
        CounterfactualError::Rejected { reason } => {
            assert_eq!(reason, RejectionReason::OtherAccount);
        }
        other => panic!("se esperaba un rechazo, no {other}"),
    }
    let message = error.to_string();
    assert!(
        !message.contains("contoso"),
        "el rechazo no describe la otra cuenta: {message}"
    );
    assert!(
        !message.contains("app-1.0.0"),
        "el rechazo no lleva sujetos: {message}"
    );
    assert_eq!(
        retriever.calls.load(Ordering::SeqCst),
        0,
        "el rechazo no gasto lecturas"
    );
}

/// A hypothesis that expired is rejected.
#[tokio::test]
async fn an_expired_hypothesis_is_rejected() {
    let graph = the_world();
    let (investigator, _retriever) = the_investigator(7);
    let service = CounterfactualService::new(investigator, the_simulation(graph));

    let long_ago = the_now() - chrono::Duration::hours(48);
    let expired = SimulationFork::new(
        "sim-93",
        Checkpoint::new("checkpoint-93", "acme", long_ago, Some(42), 7),
        the_permissive_policy(),
        long_ago,
    );
    let error = service
        .answer(
            "cf-3",
            &the_scope(),
            "why is the release blocked?",
            &expired,
            the_now(),
        )
        .await
        .expect_err("la hipotesis vivio demasiado");
    assert!(
        matches!(
            error,
            CounterfactualError::Rejected {
                reason: RejectionReason::Expired
            }
        ),
        "el motivo es la caducidad: {error}"
    );
}

/// A base against a world that no longer exists is rejected.
#[tokio::test]
async fn a_stale_base_is_rejected() {
    let graph = the_world();
    let (investigator, _retriever) = the_investigator(7);
    let service = CounterfactualService::new(investigator.clone(), the_simulation(graph.clone()));

    graph.set_the_world(vec![the_dependency(), the_advisory()], 8);
    let error = service
        .answer(
            "cf-4",
            &the_scope(),
            "why is the release blocked?",
            &the_fork(),
            the_now(),
        )
        .await
        .expect_err("el mundo se movio");
    assert!(
        matches!(
            error,
            CounterfactualError::Rejected {
                reason: RejectionReason::StaleWorld
            }
        ),
        "el motivo es el mundo viejo: {error}"
    );
}

/// An investigation and a simulation that do not agree on the version
/// of a shared source are rejected as an incoherent seal.
#[tokio::test]
async fn an_incoherent_seal_is_rejected() {
    let graph = the_world();
    // The investigator's manifest says it read generation 6;
    // the simulation computes on 7.
    let (investigator, _retriever) = the_investigator(6);
    let service = CounterfactualService::new(investigator, the_simulation(graph));

    let error = service
        .answer(
            "cf-5",
            &the_scope(),
            "why is the release blocked?",
            &the_fork(),
            the_now(),
        )
        .await
        .expect_err("las dos mitades no hablan del mismo mundo");
    assert!(
        matches!(
            error,
            CounterfactualError::Rejected {
                reason: RejectionReason::IncoherentSeal
            }
        ),
        "el motivo es el sello: {error}"
    );
    assert!(
        !error.to_string().contains("app-1.0.0"),
        "sin filtrar sujetos"
    );
}

// ───────────────────────────── AC4 ─────────────────────────────

/// The counter-history does not write: the graph and the declared position of the
/// ledger remain the same.
#[tokio::test]
async fn the_counterfactual_leaves_the_world_untouched() {
    let graph = the_world();
    let (investigator, _retriever) = the_investigator(7);
    let simulation = the_simulation(graph.clone());
    let service = CounterfactualService::new(investigator, simulation.clone());

    let before = simulation
        .graph_relations()
        .await
        .expect("el mundo de antes");
    let report = service
        .answer(
            "cf-6",
            &the_scope(),
            "why is the release blocked?",
            &the_fork(),
            the_now(),
        )
        .await
        .expect("la contrahistoria responde");
    let after = simulation
        .graph_relations()
        .await
        .expect("el mundo de despues");

    assert_eq!(before, after, "el grafo autoritativo no se movio");
    assert_eq!(graph.the_generation(), 7, "la generacion es la misma");
    assert_eq!(
        report.base.declared_ledger_position,
        Some(42),
        "la posicion declarada del ledger sigue siendo la declarada"
    );
    // And the hypothetical does not contaminate the observed.
    assert!(
        !report
            .simulation
            .observed
            .relations
            .iter()
            .any(|relation| relation.to == lib_v2()),
        "lib-v2 no existe en el mundo observado"
    );
}

/// The report is serializable and states its version: a consumer knows
/// which contract it is reading.
#[tokio::test]
async fn the_report_travels_as_json_with_its_version() {
    let (report, _graph) = the_answer_of_the_standard_world().await;
    let json = serde_json::to_string(&report).expect("el informe se serializa");
    assert!(
        json.contains("\"version\":\"1.0.0\""),
        "el JSON declara la version"
    );
    assert!(
        json.contains("\"label\":\"hypothetical\""),
        "las clases viajan en el JSON"
    );
}
