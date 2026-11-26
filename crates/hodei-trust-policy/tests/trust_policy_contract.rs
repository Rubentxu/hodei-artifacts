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
    evaluate, simulate, Activation, AuthorizationResult, DecidedBy, ExceptionFact, Exposure,
    Outcome, PolicyRegistry, PromotionGate, ReasonCode, Requirement, RequirementKind, RiskFacts,
    TargetEnvironment, TrustContext, TrustPolicy, POLICY_SCHEMA_VERSION, TRUST_CONTEXT_VERSION,
};
use kernel::Hrn;

/// The subject of the test.
fn subject() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

/// The evidence of a release that has everything.
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

/// A piece of evidence.
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

/// The capsule of a release that has everything.
fn complete_capsule() -> hodei_evidence::domain::capsule::TrustCapsule {
    CapsuleAssembler::new().assemble(&subject(), &complete_evidence(), Utc::now())
}

/// The production policy.
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

/// The context of a release that has everything.
fn complete_context() -> TrustContext {
    TrustContext::builder(&complete_capsule())
        .with_risk(RiskFacts {
            critical_known: 0,
            critical_reachable: 0,
            exposure: Exposure::Internal,
            vex_accepted: 0,
            established: true,
            risk_version: 0,
        })
        .build()
}

// ───────────────────────────── AC1 ─────────────────────────────

/// Authorization and trust are evaluated separately, and the result
/// says which of the two decided.
#[test]
fn the_authorization_and_the_trust_are_evaluated_separately() {
    let policy = production_policy();
    let gate = PromotionGate::new(policy.clone());
    let context = complete_context();

    // The principal is not allowed: the decision is the authorization's,
    // and its reason is an authorization reason.
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

    // The principal is allowed, but the artifact is missing the
    // inventory: the decision is the trust's.
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

    // And when both are satisfied, neither of the two decided.
    let allowed = gate.evaluate(AuthorizationResult::Permit, &context);
    assert_eq!(allowed.outcome, Outcome::Allow);
    assert_eq!(allowed.decided_by, DecidedBy::Neither);
}

/// An authorization that could not decide is not a permission.
#[test]
fn an_indeterminate_authorization_does_not_open_the_door() {
    let gate = PromotionGate::new(production_policy());
    let result = gate.evaluate(AuthorizationResult::Indeterminate, &complete_context());
    assert_eq!(result.outcome, Outcome::Deny);
    assert_eq!(result.decided_by, DecidedBy::Authorization);
    assert!(result.reasons().contains(&ReasonCode::AuthzIndeterminate));
}

// ───────────────────────────── AC2 ─────────────────────────────

/// The context is versioned and deterministic: the same context produces the
/// same evaluation.
#[test]
fn the_context_is_versioned_and_the_evaluation_is_deterministic() {
    let context = complete_context();
    assert_eq!(context.version, TRUST_CONTEXT_VERSION);

    let policy = production_policy();
    let first = evaluate(&context, &policy);
    let second = evaluate(&context, &policy);
    assert_eq!(first, second, "the same context gives the same evaluation");

    // And two contexts with the same content are the same context.
    let other = TrustContext::builder(&complete_capsule())
        .with_risk(RiskFacts {
            critical_known: 0,
            critical_reachable: 0,
            exposure: Exposure::Internal,
            vex_accepted: 0,
            established: true,
            risk_version: 0,
        })
        .build();
    assert_eq!(
        evaluate(&other, &policy).outcome,
        first.outcome,
        "and the same facts give the same outcome"
    );
}

/// The decision says which version of the context produced it.
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

/// The trust decision is immutable and carries reason codes, not
/// free text.
#[test]
fn the_decision_is_immutable_and_carries_reason_codes() {
    let policy = production_policy();
    let decision = evaluate(&complete_context(), &policy);
    assert_eq!(decision.outcome, Outcome::Allow);
    assert!(
        decision.reasons.is_empty(),
        "a decision that allows has no reason against it"
    );

    // The reason is a code from a closed set, and the code can be
    // read back.
    let missing = evaluate(&missing_sbom_context(), &policy);
    assert_eq!(missing.outcome, Outcome::Deny);
    assert!(missing.has_reason(ReasonCode::TrustSbomMissing));
    let json = serde_json::to_string(&missing).expect("the decision is serialized");
    assert!(
        json.contains("TRUST_SBOM_MISSING"),
        "the reason travels as a code: {json}"
    );
}

/// The decision cannot be changed after it is taken.
#[test]
fn the_decision_cannot_be_changed_after_it_is_made() {
    let decision = evaluate(&complete_context(), &production_policy());
    let json = serde_json::to_string(&decision).expect("the decision is serialized");
    let read: hodei_trust_policy::TrustDecision =
        serde_json::from_str(&json).expect("the decision is read");
    assert_eq!(read, decision, "what is written is what is read");

    // And the set of requirements says what happened with each one.
    assert!(read.requirements.iter().all(|outcome| outcome.satisfied));
}

/// The capsule of a release without an inventory: the integrity is there, and the
/// inventory is not.
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

/// The context without an inventory.
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

/// The policy decides whether what is missing is a denial or a request for
/// approval, and the engine does not decide it.
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

/// The policy set is activated with history, and it is possible to know which
/// version produced a decision.
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

    // Another version is activated.
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

    // The history has both, and the active one is the last.
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

    // And the old version remains in the history: changing the policy does not
    // rewrite the past.
    let old = registry
        .activation_of("production", "1.0.0")
        .expect("the old activation is still there");
    assert_eq!(old.activated_by, "alice");
}

/// A decision says which version of the policy produced it, and that version
/// can be resolved afterwards.
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

    // And after changing the policy, the old decision still names the
    // version that produced it.
    registry.activate(
        TrustPolicy::new("production", "2.0.0", Vec::new()),
        "bob",
        "the second one",
        now + Duration::hours(1),
    );
    assert_eq!(decision.policy_version, "1.0.0");
}

// ───────────────────────────── AC5 ─────────────────────────────

/// The conclusion of SPIKE-004: the trust policy is NOT expressed in
/// Cedar, and the boundary is written down.
#[test]
fn the_trust_policy_is_not_written_in_cedar_and_the_boundary_is_written_down() {
    // The boundary: Cedar decides WHO; the trust policy decides WHETHER
    // THE ARTIFACT COMPLIES. And the code says so, not just the documentation.
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

/// A live exception waives the requirement, and its use is recorded in the decision.
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

/// An expired exception cannot be reused.
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

/// An exception that is not the one the policy names waives nothing.
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

/// The destination environment reaches the context.
#[test]
fn the_target_environment_is_part_of_the_context() {
    let context = TrustContext::builder(&complete_capsule())
        .targeting(TargetEnvironment::Staging)
        .build();
    assert_eq!(context.target, TargetEnvironment::Staging);
    assert_eq!(context.target.as_str(), "staging");
}

/// HSP-318 (M3-UAT-06): simulating a candidate policy against the current
/// contexts names the affected releases and what the candidate says about
/// them, and changes nothing: the policy that decides stays the active one.
#[test]
fn simulating_a_candidate_policy_names_the_affected_and_activates_nothing() {
    let registry = PolicyRegistry::empty();
    registry.activate(
        production_policy(),
        "alice",
        "the policy of today",
        Utc::now(),
    );
    let contexts = vec![(subject().to_string(), complete_context())];

    // The candidate asks for a dimension the current release does not have.
    let candidate = TrustPolicy::new(
        "production",
        "2.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::TestEvidence,
            accepts: vec![DimensionState::Verified],
        })],
    );

    let active = registry.active("production");
    let report = simulate(active.as_ref(), &candidate, &contexts);

    assert_eq!(report.affected.len(), 1);
    assert_eq!(report.affected[0].subject, subject().to_string());
    assert_eq!(report.affected[0].current, Outcome::Allow);
    assert_eq!(report.affected[0].candidate, Outcome::Deny);
    assert!(report.affected[0].changed);
    assert_eq!(report.changed, vec![subject().to_string()]);
    assert_eq!(report.active_policy_version.as_deref(), Some("1.0.0"));

    // Nothing was activated: not in the report, not in the registry.
    assert!(!report.activated);
    assert_eq!(
        registry.active("production").map(|policy| policy.version),
        Some("1.0.0".to_string())
    );
    assert_eq!(registry.activation_count(), 1);
}
