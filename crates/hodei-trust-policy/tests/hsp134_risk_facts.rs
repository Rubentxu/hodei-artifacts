//! HSP-134: the risk facts a promotion is measured against.
//!
//! Two promises, both about honesty in the absence of knowledge:
//!
//! - UNKNOWN is not zero. A subject whose risk was never established does
//!   not pass a requirement that demands the absence of criticals, and it
//!   does not fail it as if a critical had been found: it says the risk
//!   is unknown, and the requirement that asked for it does not grant.
//! - The decision carries the version of the risk it measured. A verdict
//!   that does not say which version of the feed it saw cannot be
//!   reproduced against the world it judged; replaying the same context
//!   must return the same verdict and the same version.

use chrono::{Duration, Utc};
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::{ContentDigest, ContentRef};
use hodei_trust_policy::{
    evaluate, Exposure, ReasonCode, Requirement, RequirementKind, RiskFacts, TrustContext,
    TrustPolicy,
};
use kernel::Hrn;

fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn record(id: &str, kind: EvidenceType, verified: bool) -> EvidenceRecord {
    EvidenceRecord::builder(EvidenceId::new(id), subject())
        .subjects(vec![subject()])
        .evidence_type(kind)
        .source(SourceRef::by_external_id(format!("external-{id}")))
        .content(ContentRef::by_digest(ContentDigest::of_bytes(
            id.as_bytes(),
        )))
        .observed_at(Utc::now())
        .recorded_at(Utc::now())
        .verification(if verified {
            EvidenceVerification::verified("test")
        } else {
            EvidenceVerification::unverified()
        })
        .normalization(Normalization::new("test", "1.0.0"))
        .producer(Producer::new("connector", "pipelinek"))
        .build()
}

fn complete_capsule() -> hodei_evidence::domain::capsule::TrustCapsule {
    let evidence = vec![
        record("ev-integrity", EvidenceType::ArtifactIntegrity, true),
        record("ev-sbom", EvidenceType::Sbom, true),
        record("ev-provenance", EvidenceType::Provenance, true),
        record("ev-signature", EvidenceType::SignatureVerification, true),
    ];
    CapsuleAssembler::new().assemble(&subject(), &evidence, Utc::now())
}

fn risk(critical_reachable: u32, established: bool, version: u64) -> RiskFacts {
    RiskFacts {
        critical_known: critical_reachable,
        critical_reachable,
        exposure: Exposure::Internal,
        vex_accepted: 0,
        established,
        risk_version: version,
    }
}

fn risk_policy() -> TrustPolicy {
    TrustPolicy::new(
        "risk-aware",
        "1.0.0",
        vec![Requirement::denying(
            RequirementKind::NoReachableCriticalVulnerability,
        )],
    )
}

/// Lo que la vista del grafo no pudo establecer no aprueba el requisito
/// de ausencia de criticos: UNKNOWN no es cero.
#[test]
fn an_unestablished_risk_does_not_pass_the_absence_of_criticals() {
    let context = TrustContext::builder(&complete_capsule())
        .with_risk(risk(0, false, 41))
        .build();
    let decision = evaluate(&context, &risk_policy());
    assert_eq!(
        decision.reasons.first(),
        Some(&ReasonCode::RiskUnknown),
        "the refusal must name the absence of knowledge, not a critical"
    );
    assert!(!decision.outcome.advances());
}

/// Con el riesgo establecido y cero criticos, el mismo requisito pasa.
#[test]
fn an_established_zero_passes_where_the_unknown_did_not() {
    let context = TrustContext::builder(&complete_capsule())
        .with_risk(risk(0, true, 41))
        .build();
    let decision = evaluate(&context, &risk_policy());
    assert!(decision.outcome.advances());
    assert!(!decision.has_reason(ReasonCode::RiskUnknown));
}

/// La decision dice contra que version del riesgo midio, y reproducirla
/// con el mismo contexto devuelve el mismo veredicto y la misma version.
#[test]
fn the_decision_carries_the_risk_version_and_replays_with_it() {
    let context = TrustContext::builder(&complete_capsule())
        .with_risk(risk(2, true, 144))
        .assembled_at(Utc::now())
        .build();
    let policy = risk_policy();
    let first = evaluate(&context, &policy);
    assert_eq!(first.risk_version, 144);
    assert!(first.has_reason(ReasonCode::RiskReachableCriticalVulnerability));
    let second = evaluate(&context, &policy);
    assert_eq!(first.outcome, second.outcome);
    assert_eq!(first.risk_version, second.risk_version);
    assert_eq!(first.reasons, second.reasons);
}

/// Un requisito que pregunta explicitamente por el riesgo establece la
/// diferencia entre un cero medido y la ausencia de medicion.
#[test]
fn a_policy_that_asks_for_the_risk_distinguishes_zero_from_unknown() {
    let policy = TrustPolicy::new(
        "risk-required",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    );
    let unknown = TrustContext::builder(&complete_capsule())
        .with_risk(risk(0, false, 7))
        .build();
    let zero = TrustContext::builder(&complete_capsule())
        .with_risk(risk(0, true, 7))
        .build();
    assert!(!evaluate(&unknown, &policy).outcome.advances());
    assert!(evaluate(&zero, &policy).outcome.advances());
}

/// La version vive en el riesgo que el llamador trajo: el evaluador no
/// la inventa, y su ausencia (cero) se propaga tal cual.
#[test]
fn the_version_is_the_one_the_facts_carried_and_nothing_else() {
    let context = TrustContext::builder(&complete_capsule())
        .with_risk(risk(0, true, 0))
        .build();
    let decision = evaluate(&context, &risk_policy());
    assert_eq!(decision.risk_version, 0);
    let later = TrustContext::builder(&complete_capsule())
        .with_risk(risk(0, true, 9_999))
        .assembled_at(Utc::now() + Duration::seconds(1))
        .build();
    assert_eq!(evaluate(&later, &risk_policy()).risk_version, 9_999);
}
