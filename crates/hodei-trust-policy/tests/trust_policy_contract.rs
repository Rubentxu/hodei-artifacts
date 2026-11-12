//! The contract of the trust policy.
//!
//! Five things are promised and five things are checked:
//!
//! - The authorization and the trust are evaluated separately, and the
//!   result says which of the two decided.
//! - The context is versioned and deterministic: the same context gives
//!   the same evaluation.
//! - The decision is immutable and carries reason codes, not free text.
//! - The policy set is activated with history, and a decision can say
//!   which version produced it.
//! - The trust gate is not Cedar, and the boundary is written down.

use chrono::{Duration, Utc};
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::domain::capsule::{DimensionName, DimensionState};
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::{ContentDigest, ContentRef};
use hodei_trust_policy::{
    evaluate, Activation, AuthorizationResult, DecidedBy, ExceptionFact, Exposure, Outcome,
    PolicyRegistry, PromotionGate, ReasonCode, Requirement, RequirementKind, RiskFacts,
    TargetEnvironment, TrustContext, TrustPolicy, POLICY_SCHEMA_VERSION, TRUST_CONTEXT_VERSION,
};
use kernel::Hrn;

/// The subject of la prueba.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

/// The evidence of una release que lo tiene todo.
fn complete_evidence() -> Vec<EvidenceRecord> {
    let subject = subject();
    let tenant = subject.clone();
    vec![
        record(
            &tenant,
            &subject,
            "ev-integrity",
            EvidenceType::ArtifactIntegrity,
            true,
        ),
        record(&tenant, &subject, "ev-sbom", EvidenceType::Sbom, true),
        record(
            &tenant,
            &subject,
            "ev-provenance",
            EvidenceType::Provenance,
            true,
        ),
        record(
            &tenant,
            &subject,
            "ev-signature",
            EvidenceType::SignatureVerification,
            true,
        ),
    ]
}

/// Una pieza de evidencia.
fn record(
    tenant: &Hrn,
    subject: &Hrn,
    id: &str,
    kind: EvidenceType,
    verified: bool,
) -> EvidenceRecord {
    EvidenceRecord::builder(EvidenceId::new(id), tenant.clone())
        .subjects(vec![subject.clone()])
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

/// La capsula de una release que lo tiene todo.
fn complete_capsule() -> hodei_evidence::domain::capsule::TrustCapsule {
    CapsuleAssembler::new().assemble(&subject(), &complete_evidence(), Utc::now())
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

/// El contexto de una release que lo tiene todo.
fn complete_context() -> TrustContext {
    TrustContext::builder(&complete_capsule())
        .with_risk(RiskFacts {
            critical_known: 0,
            critical_reachable: 0,
            exposure: Exposure::Internal,
            vex_accepted: 0,
            established: true,
        })
        .build()
}

// ───────────────────────────── AC1 ─────────────────────────────

/// La autorizacion y la confianza se evaluan por separado, y el resultado
/// dice cual de las dos decidio.
#[test]
fn the_authorization_and_the_trust_are_evaluated_separately() {
    let policy = production_policy();
    let gate = PromotionGate::new(policy.clone());
    let context = complete_context();

    // El principal no esta permitido: la decision es de la autorizacion,
    // y su motivo es un motivo de autorizacion.
    let refused = gate.evaluate(AuthorizationResult::Deny, &context);
    assert_eq!(refused.outcome, Outcome::Deny);
    assert_eq!(refused.decided_by, DecidedBy::Authorization);
    assert!(
        refused
            .reasons()
            .contains(&ReasonCode::AuthzPrincipalNotAllowed),
        "the reason says that the principal was not allowed"
    );
    assert!(
        !refused.reasons().contains(&ReasonCode::TrustSbomMissing),
        "and it does not dress a permission failure up as a problem with the artefact"
    );

    // El principal si esta permitido, pero al artefacto le falta el
    // inventario: la decision es de la confianza.
    let context_without_sbom = TrustContext::builder(&complete_capsule())
        .with_risk(RiskFacts {
            established: true,
            exposure: Exposure::Internal,
            ..RiskFacts::default()
        })
        .build();
    let missing = TrustContext::builder(&hodei_evidence::domain::capsule::TrustCapsule {
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
        content_hash: ContentDigest::of_bytes(b"capsule"),
        assembled_at: Utc::now(),
    })
    .with_risk(RiskFacts {
        established: true,
        exposure: Exposure::Internal,
        ..RiskFacts::default()
    })
    .build();
    let _ = context_without_sbom;

    let refused_by_trust = gate.evaluate(AuthorizationResult::Permit, &missing);
    assert_eq!(refused_by_trust.outcome, Outcome::Deny);
    assert_eq!(refused_by_trust.decided_by, DecidedBy::Trust);
    assert!(
        refused_by_trust
            .reasons()
            .contains(&ReasonCode::TrustSbomMissing),
        "the reason says that the SBOM is missing"
    );
    assert!(
        !refused_by_trust
            .reasons()
            .contains(&ReasonCode::AuthzPrincipalNotAllowed),
        "and it does not blame the principal"
    );

    // Y cuando los dos estan conformes, ninguno de los dos decidio.
    let allowed = gate.evaluate(AuthorizationResult::Permit, &context);
    assert_eq!(allowed.outcome, Outcome::Allow);
    assert_eq!(allowed.decided_by, DecidedBy::Neither);
}

/// Una autorizacion que no pudo decidir no es un permiso.
#[test]
fn an_indeterminate_authorization_does_not_open_the_door() {
    let gate = PromotionGate::new(production_policy());
    let result = gate.evaluate(AuthorizationResult::Indeterminate, &complete_context());
    assert_eq!(result.outcome, Outcome::Deny);
    assert_eq!(result.decided_by, DecidedBy::Authorization);
    assert!(result.reasons().contains(&ReasonCode::AuthzIndeterminate));
}

// ───────────────────────────── AC2 ─────────────────────────────

/// El contexto es versionado y determinista: el mismo contexto produce la
/// misma evaluacion.
#[test]
fn the_context_is_versioned_and_the_evaluation_is_deterministic() {
    let context = complete_context();
    assert_eq!(context.version, TRUST_CONTEXT_VERSION);

    let policy = production_policy();
    let first = evaluate(&context, &policy);
    let second = evaluate(&context, &policy);
    assert_eq!(first, second, "the same context gives the same evaluation");

    // Y dos contextos con el mismo contenido son el mismo contexto.
    let other = TrustContext::builder(&complete_capsule())
        .with_risk(RiskFacts {
            critical_known: 0,
            critical_reachable: 0,
            exposure: Exposure::Internal,
            vex_accepted: 0,
            established: true,
        })
        .build();
    assert_eq!(
        evaluate(&other, &policy).outcome,
        first.outcome,
        "and the same facts give the same outcome"
    );
}

/// El decision dice que version del contexto lo produjo.
#[test]
fn the_decision_names_the_version_of_everything_that_took_part_in_it() {
    let policy = production_policy();
    let decision = evaluate(&complete_context(), &policy);
    assert_eq!(decision.context_version, TRUST_CONTEXT_VERSION);
    assert_eq!(decision.policy_id, policy.id);
    assert_eq!(decision.policy_version, policy.version);
    assert_eq!(decision.schema_version, POLICY_SCHEMA_VERSION);
    assert!(
        decision.capsule_hash.is_some(),
        "and it names the capsule it was measured against"
    );
    assert_eq!(decision.evidence_ids.len(), 4, "and the evidence it used");
}

// ───────────────────────────── AC3 ─────────────────────────────

/// La decision de confianza es inmutable y lleva codigos de motivo, no
/// texto libre.
#[test]
fn the_decision_is_immutable_and_carries_reason_codes() {
    let policy = production_policy();
    let decision = evaluate(&complete_context(), &policy);
    assert_eq!(decision.outcome, Outcome::Allow);
    assert!(
        decision.reasons.is_empty(),
        "a decision that allows has no reason against it"
    );

    // El motivo es un codigo de un conjunto cerrado, y el codigo se puede
    // leer de nuevo.
    let missing = evaluate(&missing_sbom_context(), &policy);
    assert_eq!(missing.outcome, Outcome::Deny);
    assert!(missing.has_reason(ReasonCode::TrustSbomMissing));
    let json = serde_json::to_string(&missing).expect("the decision is serialized");
    assert!(
        json.contains("TRUST_SBOM_MISSING"),
        "the reason travels as a code: {json}"
    );
}

/// La decision no se puede cambiar despues de tomada.
#[test]
fn the_decision_cannot_be_changed_after_it_is_made() {
    let decision = evaluate(&complete_context(), &production_policy());
    let json = serde_json::to_string(&decision).expect("the decision is serialized");
    let read: hodei_trust_policy::TrustDecision =
        serde_json::from_str(&json).expect("the decision is read");
    assert_eq!(read, decision, "what is written is what is read");

    // Y el conjunto de requisitos dice lo que paso con cada uno.
    assert!(read.requirements.iter().all(|outcome| outcome.satisfied));
}

/// La capsula de una release sin inventario: la integridad esta, y el
/// inventario no.
fn capsule_without_sbom() -> hodei_evidence::domain::capsule::TrustCapsule {
    hodei_evidence::domain::capsule::TrustCapsule {
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

/// El contexto sin inventario.
fn missing_sbom_context() -> TrustContext {
    let capsule = capsule_without_sbom();
    TrustContext::builder(&capsule)
        .with_risk(RiskFacts {
            established: true,
            exposure: Exposure::Internal,
            ..RiskFacts::default()
        })
        .build()
}

/// La politica decide si lo que falta es una negacion o una peticion de
/// aprobacion, y el motor no lo decide.
#[test]
fn the_policy_says_whether_a_failure_denies_or_asks_for_approval() {
    let denying = TrustPolicy::new(
        "denying",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::InventoryCompleteness,
            accepts: vec![DimensionState::Complete],
        })],
    );
    let approving = TrustPolicy::new(
        "approving",
        "1.0.0",
        vec![Requirement::needing_approval(RequirementKind::Dimension {
            dimension: DimensionName::InventoryCompleteness,
            accepts: vec![DimensionState::Complete],
        })],
    );

    let context = missing_sbom_context();
    assert_eq!(evaluate(&context, &denying).outcome, Outcome::Deny);
    assert_eq!(
        evaluate(&context, &approving).outcome,
        Outcome::RequireApproval,
        "the same facts, and the policy decides what they mean"
    );
    assert!(
        !evaluate(&context, &approving).advances(),
        "and an approval is not an allow"
    );
}

// ───────────────────────────── AC4 ─────────────────────────────

/// El conjunto de politicas se activa con historial, y se puede saber que
/// version produjo una decision.
#[test]
fn the_policy_set_is_activated_with_history() {
    let registry = PolicyRegistry::empty();
    let now = Utc::now();

    let first = registry.activate(
        production_policy(),
        "alice",
        "the policy of the first day",
        now,
    );
    assert_eq!(first.policy_id, "production");
    assert_eq!(first.version, "1.0.0");
    assert_eq!(first.activated_by, "alice");

    // Se activa otra version.
    let second_policy = TrustPolicy::new(
        "production",
        "2.0.0",
        vec![Requirement::denying(RequirementKind::RiskEstablished)],
    );
    let second = registry.activate(
        second_policy,
        "bob",
        "the risk has to be known",
        now + Duration::hours(1),
    );
    assert_eq!(second.version, "2.0.0");

    // El historial tiene las dos, y la activa es la ultima.
    assert_eq!(registry.activation_count(), 2);
    let history: Vec<Activation> = registry.history();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].version, "1.0.0");
    assert_eq!(history[1].version, "2.0.0");
    assert_eq!(
        registry
            .active("production")
            .expect("there is an active set")
            .version,
        "2.0.0"
    );

    // Y la version vieja sigue en el historial: cambiar la politica no
    // reescribe el pasado.
    let old = registry
        .activation_of("production", "1.0.0")
        .expect("the old activation is still there");
    assert_eq!(old.activated_by, "alice");
}

/// Una decision dice que version de la politica la produjo, y esa version
/// se puede resolver despues.
#[test]
fn a_decision_can_be_resolved_against_the_version_that_produced_it() {
    let registry = PolicyRegistry::empty();
    let now = Utc::now();
    registry.activate(production_policy(), "alice", "the first one", now);

    let decision = evaluate(&complete_context(), &production_policy());
    assert_eq!(decision.policy_version, "1.0.0");

    let activation = registry
        .activation_of(&decision.policy_id, &decision.policy_version)
        .expect("the version of the decision is resolvable");
    assert_eq!(activation.activated_by, "alice");

    // Y tras cambiar la politica, la decision vieja sigue nombrando la
    // version que la produjo.
    registry.activate(
        TrustPolicy::new("production", "2.0.0", Vec::new()),
        "bob",
        "the second one",
        now + Duration::hours(1),
    );
    assert_eq!(decision.policy_version, "1.0.0");
}

// ───────────────────────────── AC5 ─────────────────────────────

/// La conclusion del SPIKE-004: la politica de confianza NO se expresa en
/// Cedar, y la frontera esta escrita.
#[test]
fn the_trust_policy_is_not_written_in_cedar_and_the_boundary_is_written_down() {
    // La frontera: Cedar decide QUIEN; la politica de confianza decide SI
    // EL ARTEFACTO CUMPLE. Y el codigo lo dice, no solo la documentacion.
    let source = include_str!("../src/lib.rs");
    assert!(
        source.contains("The trust gate is not written in Cedar"),
        "the conclusion is written where the code is"
    );
    assert!(
        source.contains("who may promote this")
            && source.contains("whether the artefact qualifies"),
        "and the boundary is spelled out"
    );
    assert!(
        source.contains("reason codes"),
        "and it says why: the reason codes are a closed set that Cedar's reasons are not"
    );
}

// ───────────────────────────── La excepcion ─────────────────────────────

/// Una excepcion viva waivea el requisito, y su uso queda en la decision.
#[test]
fn an_active_exception_waives_the_requirement_and_stays_in_the_trace() {
    let policy = TrustPolicy::new(
        "production",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::InventoryCompleteness,
            accepts: vec![DimensionState::Complete],
        })
        .overridable_by("sbom-waiver")],
    );

    let now = Utc::now();
    let context = TrustContext::builder(&capsule_without_sbom())
        .with_risk(RiskFacts {
            established: true,
            exposure: Exposure::Internal,
            ..RiskFacts::default()
        })
        .with_exceptions(vec![ExceptionFact {
            id: "ex-1".to_string(),
            name: "sbom-waiver".to_string(),
            expires_at: now + Duration::days(1),
            applies_to_subject: true,
        }])
        .assembled_at(now)
        .build();

    let decision = evaluate(&context, &policy);
    assert_eq!(
        decision.outcome,
        Outcome::Warn,
        "the exception waives the requirement, and the decision says that it was used"
    );
    assert!(
        decision.has_reason(ReasonCode::ExceptionApplied),
        "and its use is in the decision, prominently"
    );
    assert_eq!(
        decision.requirements[0].exception.as_deref(),
        Some("ex-1"),
        "and the exception is named"
    );
}

/// Una excepcion caducada no se puede reutilizar.
#[test]
fn an_expired_exception_cannot_be_reused() {
    let policy = TrustPolicy::new(
        "production",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::InventoryCompleteness,
            accepts: vec![DimensionState::Complete],
        })
        .overridable_by("sbom-waiver")],
    );

    let now = Utc::now();
    let context = TrustContext::builder(&capsule_without_sbom())
        .with_risk(RiskFacts {
            established: true,
            exposure: Exposure::Internal,
            ..RiskFacts::default()
        })
        .with_exceptions(vec![ExceptionFact {
            id: "ex-1".to_string(),
            name: "sbom-waiver".to_string(),
            expires_at: now - Duration::hours(1),
            applies_to_subject: true,
        }])
        .assembled_at(now)
        .build();

    let decision = evaluate(&context, &policy);
    assert_eq!(
        decision.outcome,
        Outcome::Deny,
        "an exception that ran out is not a waiver"
    );
    assert!(decision.has_reason(ReasonCode::TrustSbomMissing));
    assert!(
        !decision.has_reason(ReasonCode::ExceptionApplied),
        "and it is not claimed"
    );
}

/// Una excepcion que no es la que la politica nombra no waivea nada.
#[test]
fn only_the_named_exception_waives_the_requirement() {
    let policy = TrustPolicy::new(
        "production",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::InventoryCompleteness,
            accepts: vec![DimensionState::Complete],
        })
        .overridable_by("sbom-waiver")],
    );

    let now = Utc::now();
    let context = TrustContext::builder(&capsule_without_sbom())
        .with_risk(RiskFacts {
            established: true,
            exposure: Exposure::Internal,
            ..RiskFacts::default()
        })
        .with_exceptions(vec![ExceptionFact {
            id: "ex-other".to_string(),
            name: "some-other-waiver".to_string(),
            expires_at: now + Duration::days(1),
            applies_to_subject: true,
        }])
        .assembled_at(now)
        .build();

    assert_eq!(evaluate(&context, &policy).outcome, Outcome::Deny);
}

/// El entorno de destino llega al contexto.
#[test]
fn the_target_environment_is_part_of_the_context() {
    let context = TrustContext::builder(&complete_capsule())
        .targeting(TargetEnvironment::Staging)
        .build();
    assert_eq!(context.target, TargetEnvironment::Staging);
    assert_eq!(context.target.as_str(), "staging");
}
