//! The contract of the promotion.
//!
//! Five things are promised and five things are checked:
//!
//! - The combined gate decides with authorization and trust separate, and a
//!   no from either blocks.
//! - The decision is persisted with its policy, its context, its inputs
//!   and its reasons, and it can be consulted later.
//! - The mandatory evidence that is missing produces indeterminate and not
//!   a promotion.
//! - An exception has request, approval and expiry, and an expired
//!   exception is worth nothing.
//! - Without a promotion target, the mutation answers NotSupported, and
//!   not an invented success.

use chrono::{Duration, Utc};
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::domain::capsule::{DimensionName, DimensionState, TrustCapsule};
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::{ContentDigest, ContentRef};
use hodei_trust_policy::gate::{AuthorizationResult, DecidedBy};
use hodei_trust_policy::ports::{DecisionStore, PromotionTargets};
use hodei_trust_policy::promotion::{
    DecisionId, PromotionDecision, PromotionFailure, PromotionOutcome, PromotionRequest,
    PromotionTarget,
};
use hodei_trust_policy::reasons::ReasonCode;
use hodei_trust_policy::{
    evaluate, ApprovalOutcome, ExceptionRequest, ExceptionState, Outcome, PromotionGate,
    PromotionService, Requirement, RequirementKind, RiskFacts, TrustContext, TrustPolicy,
};
use hodei_trust_policy_storage::{StaticPromotionTargets, SurrealDecisionStore};
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

/// El sujeto de la prueba.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

/// Una pieza de evidencia.
fn record(id: &str, kind: EvidenceType) -> EvidenceRecord {
    EvidenceRecord::builder(EvidenceId::new(id), subject())
        .subjects(vec![subject()])
        .evidence_type(kind)
        .source(SourceRef::by_external_id(format!("external-{id}")))
        .content(ContentRef::by_digest(ContentDigest::of_bytes(
            id.as_bytes(),
        )))
        .observed_at(Utc::now())
        .recorded_at(Utc::now())
        .verification(EvidenceVerification::verified("test"))
        .normalization(Normalization::new("test", "1.0.0"))
        .producer(Producer::new("connector", "pipelinek"))
        .build()
}

/// La capsula de una release completa.
fn complete_capsule() -> TrustCapsule {
    CapsuleAssembler::new().assemble(
        &subject(),
        &[
            record("ev-integrity", EvidenceType::ArtifactIntegrity),
            record("ev-sbom", EvidenceType::Sbom),
            record("ev-provenance", EvidenceType::Provenance),
            record("ev-signature", EvidenceType::SignatureVerification),
        ],
        Utc::now(),
    )
}

/// La capsula de una release sin inventario.
fn capsule_without_sbom() -> TrustCapsule {
    TrustCapsule {
        subject: subject(),
        profile: hodei_evidence::domain::capsule::TrustProfile {
            subject: subject(),
            dimensions: vec![hodei_evidence::domain::capsule::Dimension {
                name: DimensionName::Integrity,
                state: DimensionState::Verified,
                evidence: Vec::new(),
                reason: None,
            }],
        },
        version: "1.0.0".to_string(),
        content_hash: ContentDigest::of_bytes(b"capsule-without-sbom"),
        assembled_at: Utc::now(),
    }
}

/// La politica de produccion.
fn production_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "1.0.0",
        vec![
            Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::Integrity,
                accepts: vec![DimensionState::Verified],
            }),
            Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::InventoryCompleteness,
                accepts: vec![DimensionState::Complete],
            }),
            Requirement::denying(RequirementKind::RiskEstablished),
        ],
    )
}

/// El contexto de una release completa.
fn complete_context() -> TrustContext {
    TrustContext::builder(&complete_capsule())
        .with_risk(RiskFacts {
            established: true,
            exposure: hodei_trust_policy::Exposure::Internal,
            ..RiskFacts::default()
        })
        .build()
}

/// El almacen y el servicio.
async fn service(with_target: bool) -> (Arc<SurrealDecisionStore<Db>>, PromotionService) {
    let tmp = tempfile::tempdir().expect("a throwaway directory");
    let path = tmp.keep();
    let db = Arc::new(
        Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei")
        .use_db("trust")
        .await
        .expect("the namespace");

    let store = Arc::new(SurrealDecisionStore::new(db));
    let targets: Arc<dyn PromotionTargets> = if with_target {
        Arc::new(StaticPromotionTargets::new(vec![PromotionTarget {
            distribution: "production".to_string(),
            account: "acme".to_string(),
        }]))
    } else {
        Arc::new(StaticPromotionTargets::default())
    };
    let service = PromotionService::new(store.clone(), targets);
    (store, service)
}

/// Una peticion de promocion.
fn request() -> PromotionRequest {
    PromotionRequest::new(
        "corr-1",
        "hrn:hodei:iam::acme:User/alice",
        "acme",
        subject().to_string(),
        "production",
        Utc::now(),
    )
}

// ───────────────────────────── AC1 ─────────────────────────────

/// La puerta combinada decide con autorizacion y confianza separadas, y un
/// no de cualquiera de las dos bloquea.
#[tokio::test]
async fn a_no_from_either_half_blocks_the_promotion() {
    let (_store, service) = service(true).await;
    let gate = PromotionGate::new(production_policy());

    // El principal no esta permitido: no se promueve.
    let refused = service
        .request_promotion(
            DecisionId::new("dec-1"),
            request(),
            AuthorizationResult::Deny,
            complete_context(),
            gate.clone(),
        )
        .await
        .expect("the promotion runs");
    assert!(!refused.promoted());

    let decision = service
        .decision(&DecisionId::new("dec-1"))
        .await
        .expect("the decision is read")
        .expect("the decision is there");
    assert_eq!(decision.decided_by, DecidedBy::Authorization);
    assert_eq!(decision.outcome, Outcome::Deny);
    assert!(!decision.applied);

    // El principal si esta permitido, pero al artefacto le falta el
    // inventario: tampoco se promueve, y lo decide la confianza.
    let context = TrustContext::builder(&capsule_without_sbom())
        .with_risk(RiskFacts {
            established: true,
            exposure: hodei_trust_policy::Exposure::Internal,
            ..RiskFacts::default()
        })
        .build();
    let refused_by_trust = service
        .request_promotion(
            DecisionId::new("dec-2"),
            request(),
            AuthorizationResult::Permit,
            context,
            gate,
        )
        .await
        .expect("the promotion runs");
    assert!(!refused_by_trust.promoted());

    let decision = service
        .decision(&DecisionId::new("dec-2"))
        .await
        .expect("the decision is read")
        .expect("the decision is there");
    assert_eq!(decision.decided_by, DecidedBy::Trust);
    assert!(decision.reasons.contains(&ReasonCode::TrustSbomMissing));
}

/// Y cuando los dos estan conformes, se promueve.
#[tokio::test]
async fn a_compliant_release_is_promoted() {
    let (_store, service) = service(true).await;
    let gate = PromotionGate::new(production_policy());

    let outcome = service
        .request_promotion(
            DecisionId::new("dec-ok"),
            request(),
            AuthorizationResult::Permit,
            complete_context(),
            gate,
        )
        .await
        .expect("the promotion runs");
    assert!(outcome.promoted());

    let decision = service
        .decision(&DecisionId::new("dec-ok"))
        .await
        .expect("the decision is read")
        .expect("the decision is there");
    assert_eq!(decision.outcome, Outcome::Allow);
    assert!(decision.applied);
    assert!(decision.advanced());
}

// ───────────────────────────── AC2 ─────────────────────────────

/// La decision se persiste con su politica, su contexto, sus entradas y sus
/// razones, y se puede consultar despues.
#[tokio::test]
async fn the_decision_is_persisted_and_can_be_consulted_later() {
    let (store, service) = service(true).await;
    let gate = PromotionGate::new(production_policy());

    service
        .request_promotion(
            DecisionId::new("dec-1"),
            request(),
            AuthorizationResult::Permit,
            complete_context(),
            gate,
        )
        .await
        .expect("the promotion runs");

    // Se lee por su identificador.
    let decision = store
        .get(&DecisionId::new("dec-1"))
        .await
        .expect("the decision is read")
        .expect("the decision is there");
    assert_eq!(decision.trust.policy_id, "production");
    assert_eq!(decision.trust.policy_version, "1.0.0");
    assert_eq!(
        decision.context.version,
        hodei_trust_policy::TRUST_CONTEXT_VERSION
    );
    assert!(decision.trust.capsule_hash.is_some());
    assert!(!decision.trust.evidence_ids.is_empty());

    // Y por la correlacion.
    let by_correlation = store
        .find_by_correlation("corr-1")
        .await
        .expect("the decision is read")
        .expect("the decision is there");
    assert_eq!(by_correlation.id, decision.id);

    // Y por el sujeto.
    let by_subject = store
        .list_for_subject("acme", &subject().to_string())
        .await
        .expect("the decisions are read");
    assert_eq!(by_subject.len(), 1);

    // Y la decision sobrevive a que se lea otra vez: el contenido travels
    // entero, no la mitad.
    let again = store
        .get(&DecisionId::new("dec-1"))
        .await
        .expect("the decision is read")
        .expect("the decision is there");
    assert_eq!(again, decision);
}

/// Y una decision que se escribe dos veces es una decision.
#[tokio::test]
async fn writing_the_same_decision_twice_leaves_one_decision() {
    let (store, service) = service(true).await;
    let gate = PromotionGate::new(production_policy());
    let context = complete_context();

    service
        .request_promotion(
            DecisionId::new("dec-1"),
            request(),
            AuthorizationResult::Permit,
            context.clone(),
            gate.clone(),
        )
        .await
        .expect("the promotion runs");
    service
        .request_promotion(
            DecisionId::new("dec-1"),
            request(),
            AuthorizationResult::Permit,
            context,
            gate,
        )
        .await
        .expect("the promotion runs");

    let decisions = store
        .list_for_subject("acme", &subject().to_string())
        .await
        .expect("the decisions are read");
    assert_eq!(decisions.len(), 1);
}

// ───────────────────────────── AC3 ─────────────────────────────

/// La evidencia obligatoria ausente produce indeterminate y no una
/// promocion.
#[tokio::test]
async fn the_missing_mandatory_evidence_produces_indeterminate() {
    let (_store, service) = service(true).await;
    let gate = PromotionGate::new(production_policy());

    let context = TrustContext::builder(&capsule_without_sbom())
        .with_risk(RiskFacts {
            established: true,
            exposure: hodei_trust_policy::Exposure::Internal,
            ..RiskFacts::default()
        })
        .build();

    let outcome = service
        .request_promotion(
            DecisionId::new("dec-missing"),
            request(),
            AuthorizationResult::Permit,
            context,
            gate,
        )
        .await
        .expect("the promotion runs");
    assert!(!outcome.promoted(), "and it is not a promotion");

    let decision = service
        .decision(&DecisionId::new("dec-missing"))
        .await
        .expect("the decision is read")
        .expect("the decision is there");
    assert_eq!(
        decision.outcome,
        Outcome::Indeterminate,
        "the criteria were never looked at, and that is not the same as a refusal"
    );
    assert!(
        decision.reasons.contains(&ReasonCode::TrustSbomMissing),
        "and the reason says which evidence was not there"
    );
}

// ───────────────────────────── AC4 ─────────────────────────────

/// Una excepcion tiene solicitud, aprobacion y caducidad, y una excepcion
/// caducada no vale.
#[tokio::test]
async fn an_exception_has_request_approval_and_expiry() {
    let now = Utc::now();
    let mut request = ExceptionRequest::new(
        "ex-1",
        "sbom-waiver",
        subject().to_string(),
        "dimension:inventory_completeness",
        "the scan is not finished",
        "alice",
    )
    .with_controls(vec!["the deployment is blocked".to_string()])
    .with_ticket("SEC-1234");

    assert_eq!(request.state, ExceptionState::Requested);
    assert!(!request.is_usable_at(now));

    // Sin caducidad no se aprueba.
    let refused = request.approve("bob", now - Duration::hours(1));
    assert!(matches!(refused, ApprovalOutcome::Refused(_)));

    // Con caducidad y con alguien distinto, se aprueba.
    let mut request = ExceptionRequest::new(
        "ex-1",
        "sbom-waiver",
        subject().to_string(),
        "dimension:inventory_completeness",
        "the scan is not finished",
        "alice",
    );
    assert_eq!(
        request.approve("bob", now + Duration::days(1)),
        ApprovalOutcome::Approved
    );
    assert_eq!(request.state, ExceptionState::Approved);
    assert!(request.is_usable_at(now));
    assert!(!request.is_usable_at(now + Duration::days(2)));
}

/// Y una excepcion caducada no waivea nada en la decision.
#[tokio::test]
async fn an_expired_exception_is_worth_nothing_in_the_decision() {
    let now = Utc::now();
    let policy = TrustPolicy::new(
        "production",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::InventoryCompleteness,
            accepts: vec![DimensionState::Complete],
        })
        .overridable_by("sbom-waiver")],
    );

    let mut exception = ExceptionRequest::new(
        "ex-1",
        "sbom-waiver",
        subject().to_string(),
        "dimension:inventory_completeness",
        "the scan is not finished",
        "alice",
    );
    exception.approve("bob", now + Duration::hours(1));

    // Antes de caducar, waivea.
    let live = TrustContext::builder(&capsule_without_sbom())
        .with_exceptions(
            exception
                .to_fact(&subject().to_string(), now)
                .into_iter()
                .collect(),
        )
        .with_risk(RiskFacts {
            established: true,
            exposure: hodei_trust_policy::Exposure::Internal,
            ..RiskFacts::default()
        })
        .assembled_at(now)
        .build();
    let decision = evaluate(&live, &policy);
    assert_eq!(decision.outcome, Outcome::Warn);
    assert!(decision.has_reason(ReasonCode::ExceptionApplied));

    // Y despues de caducar, no.
    let expired = TrustContext::builder(&capsule_without_sbom())
        .with_exceptions(
            exception
                .to_fact(&subject().to_string(), now + Duration::hours(2))
                .into_iter()
                .collect(),
        )
        .with_risk(RiskFacts {
            established: true,
            exposure: hodei_trust_policy::Exposure::Internal,
            ..RiskFacts::default()
        })
        .assembled_at(now + Duration::hours(2))
        .build();
    let decision = evaluate(&expired, &policy);
    assert_eq!(
        decision.outcome,
        Outcome::Deny,
        "an exception that ran out is not a waiver"
    );
    assert!(!decision.has_reason(ReasonCode::ExceptionApplied));
}

// ───────────────────────────── AC5 ─────────────────────────────

/// Si no existe un destino de promocion, la mutacion responde
/// NotSupported explicito, no un exito inventado.
#[tokio::test]
async fn without_a_target_the_mutation_answers_not_supported() {
    let (_store, service) = service(false).await;
    let gate = PromotionGate::new(production_policy());

    let outcome = service
        .request_promotion(
            DecisionId::new("dec-no-target"),
            request(),
            AuthorizationResult::Permit,
            complete_context(),
            gate,
        )
        .await
        .expect("the promotion runs");

    assert!(
        !outcome.promoted(),
        "there was nowhere to go, and that is not a success"
    );
    assert_eq!(
        outcome,
        PromotionOutcome::Failed(PromotionFailure::NoTarget),
        "the answer is that the operation is not supported"
    );

    // Y la decision esta escrita, con lo que paso.
    let decision = service
        .decision(&DecisionId::new("dec-no-target"))
        .await
        .expect("the decision is read")
        .expect("the decision is there");
    assert!(!decision.applied, "and it says that nothing was applied");
    assert_eq!(decision.outcome, Outcome::Allow, "the criteria were met");
}

/// Y la decision de una promocion sin destino se puede consultar igual.
#[tokio::test]
async fn the_decision_of_a_promotion_without_a_target_is_written_down() {
    let (store, service) = service(false).await;
    let gate = PromotionGate::new(production_policy());

    service
        .request_promotion(
            DecisionId::new("dec-no-target"),
            request(),
            AuthorizationResult::Permit,
            complete_context(),
            gate,
        )
        .await
        .expect("the promotion runs");

    let decision: PromotionDecision = store
        .get(&DecisionId::new("dec-no-target"))
        .await
        .expect("the decision is read")
        .expect("the decision is there");
    assert!(!decision.applied);
    assert_eq!(decision.request.distribution, "production");
}
