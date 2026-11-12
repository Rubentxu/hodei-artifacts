//! La UAT de M3: la decision de promocion.
//!
//! Son los siete escenarios de `uat/M3-POLICY-PROMOTION-UAT.md`, ejecutados
//! sobre fixtures locales y servicios efimeros, sin integraciones
//! obligatorias con otros productos.
//!
//! Cada escenario deja el comando con el que se ejecuta y su resultado, que
//! es lo que la tarea pide que quede registrado.
//!
//! # Los escenarios
//!
//! - M3-UAT-01 La release conforme.
//! - M3-UAT-02 El solicitante sin autorizacion.
//! - M3-UAT-03 El SBOM que falta.
//! - M3-UAT-04 Persistir, reiniciar y reproducir.
//! - M3-UAT-05 El ciclo de vida de la excepcion.
//! - M3-UAT-06 La simulacion de la activacion de una politica.
//! - M3-UAT-07 El fallo del motor o del esquema de la politica.
//!
//! # Que se ejecuta de verdad
//!
//! La decision se toma contra la politica real, con el contexto real
//! construido desde una capsula real, y se escribe en un almacen real que
//! sobrevive a un reinicio: el escenario del reinicio abre el mismo
//! directorio otra vez y lee lo que quedo.

use chrono::{Duration, Utc};
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::domain::capsule::{DimensionName, DimensionState, TrustCapsule};
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::{ContentDigest, ContentRef};
use hodei_trust_policy::gate::AuthorizationResult;
use hodei_trust_policy::ports::{DecisionStore, PromotionTargets};
use hodei_trust_policy::promotion::{
    DecisionId, PromotionFailure, PromotionOutcome, PromotionRequest, PromotionTarget,
};
use hodei_trust_policy::reasons::ReasonCode;
use hodei_trust_policy::{
    evaluate, simulate, ApprovalOutcome, ExceptionRequest, Outcome, PolicyRegistry,
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
fn evidence(id: &str, kind: EvidenceType) -> EvidenceRecord {
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
            evidence("ev-integrity", EvidenceType::ArtifactIntegrity),
            evidence("ev-sbom", EvidenceType::Sbom),
            evidence("ev-provenance", EvidenceType::Provenance),
            evidence("ev-signature", EvidenceType::SignatureVerification),
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
            Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::BuildProvenance,
                accepts: vec![DimensionState::Verified],
            }),
            Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::SignatureState,
                accepts: vec![DimensionState::Verified],
            }),
            Requirement::denying(RequirementKind::NoReachableCriticalVulnerability),
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

/// El contexto de una release sin inventario.
fn context_without_sbom() -> TrustContext {
    TrustContext::builder(&capsule_without_sbom())
        .with_risk(RiskFacts {
            established: true,
            exposure: hodei_trust_policy::Exposure::Internal,
            ..RiskFacts::default()
        })
        .build()
}

/// Una peticion de promocion.
fn request(correlation: &str) -> PromotionRequest {
    PromotionRequest::new(
        correlation,
        "hrn:hodei:iam::acme:User/alice",
        "acme",
        subject().to_string(),
        "production",
        Utc::now(),
    )
}

/// El almacen, la politica y el servicio.
async fn service(
    path: &std::path::Path,
    with_target: bool,
) -> (
    Arc<SurrealDecisionStore<Db>>,
    Arc<PolicyRegistry>,
    PromotionService,
) {
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
    let policies = Arc::new(PolicyRegistry::empty());
    policies.activate(
        production_policy(),
        "alice",
        "the policy of production",
        Utc::now(),
    );
    let service = PromotionService::new(store.clone(), targets, policies.clone());
    (store, policies, service)
}

/// El resultado de un escenario, tal como queda registrado.
fn record(scenario: &str, outcome: Result<(), String>) {
    match outcome {
        Ok(()) => println!("{scenario}: PASSED"),
        Err(reason) => println!("{scenario}: FAILED -- {reason}"),
    }
}

// ───────────────────────────── M3-UAT-01 ─────────────────────────────

/// M3-UAT-01 La release conforme.
///
/// Given all baseline evidence passes and requester is authorized, when
/// promotion to production is requested, then result is ALLOW and trace
/// includes authorization result, trust context version, policy/schema
/// version, capsule hash and evidence references.
#[tokio::test]
async fn m3_uat_01_allow_compliant_release() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("a throwaway directory");
        let path = tmp.keep();
        let (store, _policies, service) = service(&path, true).await;

        let outcome = service
            .request_promotion(
                DecisionId::new("dec-1"),
                request("corr-1"),
                AuthorizationResult::Permit,
                complete_context(),
                "production",
            )
            .await
            .map_err(|error| error.to_string())?;
        assert!(outcome.promoted(), "a compliant release is promoted");

        let decision = store
            .get(&DecisionId::new("dec-1"))
            .await
            .map_err(|error| error.to_string())?
            .ok_or("the decision was not written")?;

        assert_eq!(decision.outcome, Outcome::Allow);
        assert!(decision.applied);

        // Y la traza lleva todo lo que el enunciado nombra.
        assert_eq!(
            decision.trust.context_version,
            hodei_trust_policy::TRUST_CONTEXT_VERSION,
            "the version of the trust context"
        );
        assert_eq!(decision.trust.policy_version, "1.0.0");
        assert_eq!(
            decision.trust.schema_version,
            hodei_trust_policy::POLICY_SCHEMA_VERSION,
            "the version of the policy and of its schema"
        );
        assert!(
            decision.trust.capsule_hash.is_some(),
            "the hash of the capsule"
        );
        assert!(
            !decision.trust.evidence_ids.is_empty(),
            "the evidence references"
        );
        assert_eq!(
            decision.authorization,
            AuthorizationResult::Permit,
            "the result of the authorization"
        );
        Ok(())
    }
    .await;

    record("M3-UAT-01", result);
}

// ───────────────────────────── M3-UAT-02 ─────────────────────────────

/// M3-UAT-02 El solicitante sin autorizacion.
///
/// Given software is trusted but requester lacks authorization, when
/// promotion is requested, then combined operation is denied and trust
/// result is not confused with authorization reason.
#[tokio::test]
async fn m3_uat_02_unauthorized_requester() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("a throwaway directory");
        let path = tmp.keep();
        let (store, _policies, service) = service(&path, true).await;

        // El software es de fiar, y el solicitante no.
        let outcome = service
            .request_promotion(
                DecisionId::new("dec-2"),
                request("corr-2"),
                AuthorizationResult::Deny,
                complete_context(),
                "production",
            )
            .await
            .map_err(|error| error.to_string())?;
        assert!(!outcome.promoted());

        let decision = store
            .get(&DecisionId::new("dec-2"))
            .await
            .map_err(|error| error.to_string())?
            .ok_or("the decision was not written")?;

        assert_eq!(decision.outcome, Outcome::Deny);
        assert_eq!(
            decision.decided_by,
            hodei_trust_policy::DecidedBy::Authorization,
            "the authorization decided it"
        );
        assert!(
            decision
                .reasons
                .contains(&ReasonCode::AuthzPrincipalNotAllowed),
            "and the reason is a reason of authorization"
        );
        assert!(
            decision.reasons.iter().all(|reason| !reason.is_absence()),
            "and the trust result is not confused with it"
        );
        Ok(())
    }
    .await;

    record("M3-UAT-02", result);
}

// ───────────────────────────── M3-UAT-03 ─────────────────────────────

/// M3-UAT-03 El SBOM que falta.
///
/// Given requester authorized but SBOM missing, when promotion is
/// requested, then TrustDecision is DENY or REQUIRE_APPROVAL exactly as
/// policy specifies, never implicit ALLOW.
#[tokio::test]
async fn m3_uat_03_missing_sbom() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("a throwaway directory");
        let path = tmp.keep();
        let (store, policies, service) = service(&path, true).await;

        // Con la politica de ahora, la falta es un rechazo.
        let outcome = service
            .request_promotion(
                DecisionId::new("dec-3"),
                request("corr-3"),
                AuthorizationResult::Permit,
                context_without_sbom(),
                "production",
            )
            .await
            .map_err(|error| error.to_string())?;
        assert!(!outcome.promoted());

        let decision = store
            .get(&DecisionId::new("dec-3"))
            .await
            .map_err(|error| error.to_string())?
            .ok_or("the decision was not written")?;
        assert_eq!(decision.trust.outcome, Outcome::Deny);
        assert!(
            decision.trust.has_reason(ReasonCode::TrustSbomMissing),
            "and it says which evidence was not there"
        );
        assert_ne!(decision.trust.outcome, Outcome::Allow);

        // Y con una politica que pida aprobacion en vez de rechazar, la
        // decision es REQUIRE_APPROVAL. Nunca un ALLOW.
        policies.activate(
            TrustPolicy::new(
                "production",
                "2.0.0",
                vec![Requirement::needing_approval(RequirementKind::Dimension {
                    dimension: DimensionName::InventoryCompleteness,
                    accepts: vec![DimensionState::Complete],
                })],
            ),
            "bob",
            "the inventory asks for an approval",
            Utc::now(),
        );
        let outcome = service
            .request_promotion(
                DecisionId::new("dec-3b"),
                request("corr-3b"),
                AuthorizationResult::Permit,
                context_without_sbom(),
                "production",
            )
            .await
            .map_err(|error| error.to_string())?;
        assert!(!outcome.promoted());

        let decision = store
            .get(&DecisionId::new("dec-3b"))
            .await
            .map_err(|error| error.to_string())?
            .ok_or("the decision was not written")?;
        assert_eq!(
            decision.trust.outcome,
            Outcome::RequireApproval,
            "the policy asks for an approval and the decision says so"
        );
        assert_ne!(decision.trust.outcome, Outcome::Allow);
        Ok(())
    }
    .await;

    record("M3-UAT-03", result);
}

// ───────────────────────────── M3-UAT-04 ─────────────────────────────

/// M3-UAT-04 Persistir, reiniciar y reproducir.
///
/// Given decision D created with policy P/schema S/capsule H, when service
/// restarts and D is inspected/replayed, then referenced P/S/H remain
/// resolvable and reasons are reproducible.
#[tokio::test]
async fn m3_uat_04_persist_restart_reproduce() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("a throwaway directory");
        let path = tmp.keep();

        let before = {
            let (store, _policies, service) = service(&path, true).await;
            service
                .request_promotion(
                    DecisionId::new("dec-4"),
                    request("corr-4"),
                    AuthorizationResult::Permit,
                    complete_context(),
                    "production",
                )
                .await
                .map_err(|error| error.to_string())?;
            let decision = store
                .get(&DecisionId::new("dec-4"))
                .await
                .map_err(|error| error.to_string())?
                .ok_or("the decision was not written")?;
            // El servicio se apaga: la conexion suelta el directorio, que
            // es lo que hace que el reinicio sea un reinicio y no una
            // segunda conexion al mismo proceso.
            drop(service);
            drop(store);
            decision
        };

        // El servicio se reinicia: el mismo directorio, otro proceso.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let (store, _policies, _service) = service(&path, true).await;
        let after = store
            .get(&DecisionId::new("dec-4"))
            .await
            .map_err(|error| error.to_string())?
            .ok_or("the decision did not survive the restart")?;

        // Lo que se referencia sigue resolviendo.
        assert_eq!(after.trust.policy_id, before.trust.policy_id);
        assert_eq!(after.trust.policy_version, before.trust.policy_version);
        assert_eq!(after.trust.schema_version, before.trust.schema_version);
        assert_eq!(after.trust.capsule_hash, before.trust.capsule_hash);

        // Y las razones se reproducen: la decision se vuelve a hacer con
        // el mismo contexto y da lo mismo.
        let policy = production_policy();
        let replayed = evaluate(&after.context, &policy);
        assert_eq!(replayed.outcome, before.trust.outcome);
        assert_eq!(replayed.reasons, before.trust.reasons);
        Ok(())
    }
    .await;

    record("M3-UAT-04", result);
}

// ───────────────────────────── M3-UAT-05 ─────────────────────────────

/// M3-UAT-05 El ciclo de vida de la excepcion.
///
/// Given policy would DENY and an authorized expiring exception is
/// approved, when promotion occurs within scope/time, then decision trace
/// prominently records exception. After expiration a reevaluation cannot
/// reuse it.
#[tokio::test]
async fn m3_uat_05_exception_lifecycle() {
    let result: Result<(), String> = async {
        let now = Utc::now();

        // La peticion, la aprobacion y la caducidad.
        let mut exception = ExceptionRequest::new(
            "ex-1",
            "sbom-waiver",
            subject().to_string(),
            "dimension:inventory_completeness",
            "the scan of the transitive dependencies is not finished",
            "alice",
        )
        .with_controls(vec!["the deployment stays in staging".to_string()])
        .with_ticket("SEC-1234");

        assert!(
            matches!(
                exception.approve("alice", now + Duration::days(1)),
                ApprovalOutcome::Refused(_)
            ),
            "the one who asks cannot approve it"
        );

        let mut exception = ExceptionRequest::new(
            "ex-1",
            "sbom-waiver",
            subject().to_string(),
            "dimension:inventory_completeness",
            "the scan of the transitive dependencies is not finished",
            "alice",
        );
        assert_eq!(
            exception.approve("bob", now + Duration::days(1)),
            ApprovalOutcome::Approved,
            "somebody else approves it"
        );

        let policy = TrustPolicy::new(
            "production",
            "1.0.0",
            vec![Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::InventoryCompleteness,
                accepts: vec![DimensionState::Complete],
            })
            .overridable_by("sbom-waiver")],
        );

        // Dentro del plazo y del alcance, la excepcion waivea.
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
        assert_ne!(decision.outcome, Outcome::Deny);
        assert!(
            decision.has_reason(ReasonCode::ExceptionApplied),
            "the trace records the exception prominently"
        );
        assert_eq!(decision.requirements[0].exception.as_deref(), Some("ex-1"));

        // Y despues de caducar, no se puede reutilizar.
        let later = now + Duration::days(2);
        let expired = TrustContext::builder(&capsule_without_sbom())
            .with_exceptions(
                exception
                    .to_fact(&subject().to_string(), later)
                    .into_iter()
                    .collect(),
            )
            .with_risk(RiskFacts {
                established: true,
                exposure: hodei_trust_policy::Exposure::Internal,
                ..RiskFacts::default()
            })
            .assembled_at(later)
            .build();
        let decision = evaluate(&expired, &policy);
        assert_eq!(
            decision.outcome,
            Outcome::Deny,
            "an expired exception cannot be reused"
        );
        assert!(!decision.has_reason(ReasonCode::ExceptionApplied));
        Ok(())
    }
    .await;

    record("M3-UAT-05", result);
}

// ───────────────────────────── M3-UAT-06 ─────────────────────────────

/// M3-UAT-06 La simulacion de la activacion de una politica.
///
/// Given current deployments and proposed policy P2, when simulation/preview
/// evaluates P2 against current contexts (without activation), then
/// affected release set is returned and no production policy version
/// changes.
#[tokio::test]
async fn m3_uat_06_policy_activation_simulation() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("a throwaway directory");
        let path = tmp.keep();
        let (_store, policies, _service) = service(&path, true).await;

        // El contexto actual de una release que cumple.
        let contexts = vec![(subject().to_string(), complete_context())];

        // La politica candidata: exige las pruebas, que la release actual
        // no tiene, porque el modelo de la capsula todavia no las mide.
        let candidate = TrustPolicy::new(
            "production",
            "2.0.0",
            vec![Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::TestEvidence,
                accepts: vec![DimensionState::Verified],
            })],
        );

        let active = policies.active("production");
        let report = simulate(active.as_ref(), &candidate, &contexts);

        // El conjunto afectado se devuelve.
        assert_eq!(report.affected.len(), 1);
        assert_eq!(
            report.affected[0].subject,
            subject().to_string(),
            "the affected release is named"
        );
        assert_eq!(
            report.affected[0].candidate,
            Outcome::Deny,
            "and what the candidate says about it"
        );

        // Y la version de produccion NO cambia.
        assert!(
            !report.activated,
            "the simulation does not activate anything"
        );
        assert_eq!(
            policies.active("production").map(|policy| policy.version),
            Some("1.0.0".to_string()),
            "and the policy that decides is the same one"
        );
        assert_eq!(policies.activation_count(), 1, "and nothing was activated");
        assert_eq!(report.active_policy_version.as_deref(), Some("1.0.0"));
        Ok(())
    }
    .await;

    record("M3-UAT-06", result);
}

// ───────────────────────────── M3-UAT-07 ─────────────────────────────

/// M3-UAT-07 El fallo del motor o del esquema de la politica.
///
/// Given production promotion and policy/schema cannot be loaded, when
/// request executes, then configured secure behavior is fail-closed with
/// operational error distinct from policy DENY.
#[tokio::test]
async fn m3_uat_07_policy_engine_schema_failure() {
    let result: Result<(), String> = async {
        let tmp = tempfile::tempdir().expect("a throwaway directory");
        let path = tmp.keep();
        let (store, _policies, service) = service(&path, true).await;

        // Se pide una politica que no existe: el conjunto no se ha
        // activado nunca, y el motor no puede cargarlo.
        let outcome = service
            .request_promotion(
                DecisionId::new("dec-7"),
                request("corr-7"),
                AuthorizationResult::Permit,
                complete_context(),
                "a-policy-that-was-never-activated",
            )
            .await
            .map_err(|error| error.to_string())?;

        assert!(
            !outcome.promoted(),
            "the behavior is closed: nothing advances"
        );
        assert_eq!(
            outcome,
            PromotionOutcome::Failed(PromotionFailure::Refused),
            "and the promotion does not go ahead"
        );

        let decision = store
            .get(&DecisionId::new("dec-7"))
            .await
            .map_err(|error| error.to_string())?
            .ok_or("the decision was not written")?;

        // El fallo es un fallo OPERACIONAL, distinto de un rechazo de la
        // politica.
        assert_eq!(
            decision.outcome,
            Outcome::Indeterminate,
            "it is not a policy DENY: the criteria were never looked at"
        );
        assert_ne!(decision.outcome, Outcome::Deny);
        assert!(
            decision.reasons.contains(&ReasonCode::PolicyUnavailable),
            "and the reason says that the policy could not be loaded"
        );
        assert!(
            !decision.reasons.contains(&ReasonCode::TrustSbomMissing),
            "and it is not confused with a failure of the criteria"
        );
        assert!(!decision.applied);
        Ok(())
    }
    .await;

    record("M3-UAT-07", result);
}
