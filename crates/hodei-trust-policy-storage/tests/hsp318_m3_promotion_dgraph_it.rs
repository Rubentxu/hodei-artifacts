//! HSP-318: the M3 promotion-decision UAT (HSP-63) rewritten on Dgraph.
//!
//! The retired `m3_uat` ran the promotion service over the embedded engine.
//! This IT keeps the scenarios whose oracle depends on the DURABLE store and
//! runs them through the real `PromotionService` into `DgraphDecisionStore`:
//!
//! - M3-UAT-01 a compliant release is allowed and the persisted trace names
//!   the authorization result, the context/policy/schema versions, the
//!   capsule hash and the evidence references.
//! - M3-UAT-02 an unauthorized requester is denied by authorization, and the
//!   trust result is not confused with the authorization reason.
//! - M3-UAT-03 a missing SBOM is DENY, or REQUIRE_APPROVAL when the policy
//!   says so, never an implicit ALLOW.
//! - M3-UAT-04 persist, restart and replay: a brand-new connection and store
//!   read the decision back, what it references still resolves and the
//!   reasons are reproduced by re-evaluating the stored context.
//! - M3-UAT-07 a policy that cannot be loaded is a closed operational
//!   failure (INDETERMINATE), distinct from a policy DENY.
//!
//! The exception lifecycle (05) and the activation simulation (06) are pure
//! domain behavior and live in `hodei-trust-policy` (`trust_policy_contract`).
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack; without it
//! each test skips itself. Run serially against the shared alpha:
//! `cargo test -p hodei-trust-policy-storage --test
//! hsp318_m3_promotion_dgraph_it -- --test-threads=1`.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use hodei_evidence::application::capsule::CapsuleAssembler;
use hodei_evidence::domain::capsule::{
    Dimension, DimensionName, DimensionState, TrustCapsule, TrustProfile,
};
use hodei_evidence::domain::evidence::{
    EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization, Producer,
    SourceRef,
};
use hodei_evidence::{ContentDigest, ContentRef};
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_trust_policy::gate::AuthorizationResult;
use hodei_trust_policy::ports::{DecisionStore, PromotionTargets};
use hodei_trust_policy::promotion::{
    DecisionId, PromotionFailure, PromotionOutcome, PromotionRequest, PromotionTarget,
};
use hodei_trust_policy::reasons::ReasonCode;
use hodei_trust_policy::{
    evaluate, DecidedBy, Exposure, Outcome, PolicyRegistry, PromotionService, Requirement,
    RequirementKind, RiskFacts, TrustContext, TrustPolicy,
};
use hodei_trust_policy_storage::{DgraphDecisionStore, StaticPromotionTargets};
use kernel::Hrn;

/// The env configuration of the stack ITs, exactly as the other ITs read it.
fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

/// A fresh connection to the shared authority, with the bounded open retry
/// the shared server sometimes needs.
async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut last = String::new();
    for attempt in 1..=3 {
        let mut client = match GraphClient::connect(config.clone()).await {
            Ok(client) => client,
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
                continue;
            }
        };
        match client.login().await {
            Ok(()) => return Some(Arc::new(client)),
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(Duration::from_millis(250 * attempt as u64)).await;
            }
        }
    }
    panic!("Dgraph did not open after 3 attempts: {last}");
}

/// A scope unique to this invocation: the account, the subject and every
/// identifier derive from it, so two runs or two tests never read each
/// other's decisions on the shared alpha.
fn run_scope() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static RUN: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock moves")
        .as_nanos() as u64;
    format!("hsp318-{}-{}", nanos, RUN.fetch_add(1, Ordering::SeqCst))
}

/// The subject of the scenarios, inside the scope's account.
fn subject(scope: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{scope}:Artifact/web-1.0.0"
    ))
    .expect("an HRN")
}

fn evidence(scope: &str, id: &str, kind: EvidenceType) -> EvidenceRecord {
    let subject = subject(scope);
    EvidenceRecord::builder(EvidenceId::new(format!("{scope}-{id}")), subject.clone())
        .subjects(vec![subject])
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

/// The capsule of a complete release.
fn complete_capsule(scope: &str) -> TrustCapsule {
    CapsuleAssembler::new().assemble(
        &subject(scope),
        &[
            evidence(scope, "ev-integrity", EvidenceType::ArtifactIntegrity),
            evidence(scope, "ev-sbom", EvidenceType::Sbom),
            evidence(scope, "ev-provenance", EvidenceType::Provenance),
            evidence(scope, "ev-signature", EvidenceType::SignatureVerification),
        ],
        Utc::now(),
    )
}

/// The capsule of a release without an inventory.
fn capsule_without_sbom(scope: &str) -> TrustCapsule {
    TrustCapsule {
        subject: subject(scope),
        profile: TrustProfile {
            subject: subject(scope),
            dimensions: vec![Dimension {
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

fn dimension(name: DimensionName, accepts: DimensionState) -> RequirementKind {
    RequirementKind::Dimension {
        dimension: name,
        accepts: vec![accepts],
    }
}

/// The production policy.
fn production_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "1.0.0",
        vec![
            Requirement::denying(dimension(
                DimensionName::Integrity,
                DimensionState::Verified,
            )),
            Requirement::denying(dimension(
                DimensionName::InventoryCompleteness,
                DimensionState::Complete,
            )),
            Requirement::denying(dimension(
                DimensionName::BuildProvenance,
                DimensionState::Verified,
            )),
            Requirement::denying(dimension(
                DimensionName::SignatureState,
                DimensionState::Verified,
            )),
            Requirement::denying(RequirementKind::NoReachableCriticalVulnerability),
            Requirement::denying(RequirementKind::RiskEstablished),
        ],
    )
}

fn established_risk() -> RiskFacts {
    RiskFacts {
        established: true,
        exposure: Exposure::Internal,
        ..RiskFacts::default()
    }
}

fn complete_context(scope: &str) -> TrustContext {
    TrustContext::builder(&complete_capsule(scope))
        .with_risk(established_risk())
        .build()
}

fn context_without_sbom(scope: &str) -> TrustContext {
    TrustContext::builder(&capsule_without_sbom(scope))
        .with_risk(established_risk())
        .build()
}

fn request(scope: &str, correlation: &str) -> PromotionRequest {
    PromotionRequest::new(
        format!("{scope}-{correlation}"),
        "hrn:hodei:iam::acme:User/alice",
        scope,
        subject(scope).to_string(),
        "production",
        Utc::now(),
    )
}

/// The Dgraph decision store, the active policy registry and the service.
async fn service(
    client: Arc<GraphClient>,
    scope: &str,
) -> (
    Arc<DgraphDecisionStore>,
    Arc<PolicyRegistry>,
    PromotionService,
) {
    let store = Arc::new(
        DgraphDecisionStore::open(client)
            .await
            .expect("the decision store opens"),
    );
    let targets: Arc<dyn PromotionTargets> =
        Arc::new(StaticPromotionTargets::new(vec![PromotionTarget {
            distribution: "production".to_string(),
            account: scope.to_string(),
        }]));
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

/// M3-UAT-01: a compliant release is allowed and the persisted trace carries
/// everything the scenario names.
#[tokio::test]
async fn m3_uat_01_allow_compliant_release_persists_the_full_trace() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let scope = run_scope();
    let (store, _policies, service) = service(client, &scope).await;
    let id = DecisionId::new(format!("{scope}-dec-1"));

    let outcome = service
        .request_promotion(
            id.clone(),
            request(&scope, "corr-1"),
            AuthorizationResult::Permit,
            complete_context(&scope),
            "production",
        )
        .await
        .expect("the promotion is decided");
    assert!(outcome.promoted(), "a compliant release is promoted");

    let decision = store
        .get(&id)
        .await
        .expect("the store reads")
        .expect("the decision was written");
    assert_eq!(decision.outcome, Outcome::Allow);
    assert!(decision.applied);
    assert_eq!(
        decision.trust.context_version,
        hodei_trust_policy::TRUST_CONTEXT_VERSION
    );
    assert_eq!(decision.trust.policy_version, "1.0.0");
    assert_eq!(
        decision.trust.schema_version,
        hodei_trust_policy::POLICY_SCHEMA_VERSION
    );
    assert!(
        decision.trust.capsule_hash.is_some(),
        "the hash of the capsule"
    );
    assert!(
        !decision.trust.evidence_ids.is_empty(),
        "the evidence references"
    );
    assert_eq!(decision.authorization, AuthorizationResult::Permit);
}

/// M3-UAT-02: the software is trusted and the requester is not; the
/// authorization decides and its reason is not a trust reason.
#[tokio::test]
async fn m3_uat_02_unauthorized_requester_is_denied_by_authorization() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let scope = run_scope();
    let (store, _policies, service) = service(client, &scope).await;
    let id = DecisionId::new(format!("{scope}-dec-2"));

    let outcome = service
        .request_promotion(
            id.clone(),
            request(&scope, "corr-2"),
            AuthorizationResult::Deny,
            complete_context(&scope),
            "production",
        )
        .await
        .expect("the promotion is decided");
    assert!(!outcome.promoted());

    let decision = store
        .get(&id)
        .await
        .expect("the store reads")
        .expect("the decision was written");
    assert_eq!(decision.outcome, Outcome::Deny);
    assert_eq!(decision.decided_by, DecidedBy::Authorization);
    assert!(
        decision
            .reasons
            .contains(&ReasonCode::AuthzPrincipalNotAllowed),
        "the reason is a reason of authorization"
    );
    assert!(
        decision.reasons.iter().all(|reason| !reason.is_absence()),
        "the trust result is not confused with it"
    );
}

/// M3-UAT-03: a missing SBOM is DENY, or REQUIRE_APPROVAL when the active
/// policy asks for it; never an implicit ALLOW.
#[tokio::test]
async fn m3_uat_03_missing_sbom_is_deny_or_approval_never_allow() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let scope = run_scope();
    let (store, policies, service) = service(client, &scope).await;

    let denied = DecisionId::new(format!("{scope}-dec-3"));
    let outcome = service
        .request_promotion(
            denied.clone(),
            request(&scope, "corr-3"),
            AuthorizationResult::Permit,
            context_without_sbom(&scope),
            "production",
        )
        .await
        .expect("the promotion is decided");
    assert!(!outcome.promoted());
    let decision = store
        .get(&denied)
        .await
        .expect("the store reads")
        .expect("the decision was written");
    assert_eq!(decision.trust.outcome, Outcome::Deny);
    assert!(
        decision.trust.has_reason(ReasonCode::TrustSbomMissing),
        "it says which evidence was not there"
    );

    policies.activate(
        TrustPolicy::new(
            "production",
            "2.0.0",
            vec![Requirement::needing_approval(dimension(
                DimensionName::InventoryCompleteness,
                DimensionState::Complete,
            ))],
        ),
        "bob",
        "the inventory asks for an approval",
        Utc::now(),
    );
    let approval = DecisionId::new(format!("{scope}-dec-3b"));
    let outcome = service
        .request_promotion(
            approval.clone(),
            request(&scope, "corr-3b"),
            AuthorizationResult::Permit,
            context_without_sbom(&scope),
            "production",
        )
        .await
        .expect("the promotion is decided");
    assert!(!outcome.promoted());
    let decision = store
        .get(&approval)
        .await
        .expect("the store reads")
        .expect("the decision was written");
    assert_eq!(
        decision.trust.outcome,
        Outcome::RequireApproval,
        "the policy asks for an approval and the decision says so"
    );
    assert_eq!(
        decision.trust.policy_version, "2.0.0",
        "and it names the version of the policy that asked"
    );
}

/// M3-UAT-04: persist, restart, replay. The "restart" is a brand-new
/// connection and store over the same authority: nothing is shared with the
/// service that wrote the decision.
#[tokio::test]
async fn m3_uat_04_decision_survives_a_restart_and_replays() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let scope = run_scope();
    let id = DecisionId::new(format!("{scope}-dec-4"));

    let before = {
        let (store, _policies, service) = service(client, &scope).await;
        service
            .request_promotion(
                id.clone(),
                request(&scope, "corr-4"),
                AuthorizationResult::Permit,
                complete_context(&scope),
                "production",
            )
            .await
            .expect("the promotion is decided");
        store
            .get(&id)
            .await
            .expect("the store reads")
            .expect("the decision was written")
    };

    // The service restarts: another connection, another store.
    let restarted = fresh_client().await.expect("the alpha is still there");
    let store = DgraphDecisionStore::open(restarted)
        .await
        .expect("the decision store reopens");
    let after = store
        .get(&id)
        .await
        .expect("the store reads")
        .expect("the decision did not survive the restart");

    assert_eq!(after.trust.policy_id, before.trust.policy_id);
    assert_eq!(after.trust.policy_version, before.trust.policy_version);
    assert_eq!(after.trust.schema_version, before.trust.schema_version);
    assert_eq!(after.trust.capsule_hash, before.trust.capsule_hash);

    // The reasons are reproducible: the stored context, evaluated again
    // against the policy it names, gives the same verdict.
    let replayed = evaluate(&after.context, &production_policy());
    assert_eq!(replayed.outcome, before.trust.outcome);
    assert_eq!(replayed.reasons, before.trust.reasons);

    // And the decision is findable by its subject after the restart.
    let listed = store
        .list_for_subject(&scope, &subject(&scope).to_string())
        .await
        .expect("the subject reads");
    assert_eq!(listed.len(), 1);
}

/// M3-UAT-07: a policy that cannot be loaded is a CLOSED operational
/// failure (INDETERMINATE) distinct from a policy DENY, and is written down.
#[tokio::test]
async fn m3_uat_07_unloadable_policy_fails_closed_as_indeterminate() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let scope = run_scope();
    let (store, _policies, service) = service(client, &scope).await;
    let id = DecisionId::new(format!("{scope}-dec-7"));

    let outcome = service
        .request_promotion(
            id.clone(),
            request(&scope, "corr-7"),
            AuthorizationResult::Permit,
            complete_context(&scope),
            "a-policy-that-was-never-activated",
        )
        .await
        .expect("the promotion is decided");
    assert_eq!(
        outcome,
        PromotionOutcome::Failed(PromotionFailure::Refused),
        "nothing advances"
    );

    let decision = store
        .get(&id)
        .await
        .expect("the store reads")
        .expect("the decision was written");
    assert_eq!(decision.outcome, Outcome::Indeterminate);
    assert_ne!(decision.outcome, Outcome::Deny);
    assert!(decision.reasons.contains(&ReasonCode::PolicyUnavailable));
    assert!(
        !decision.reasons.contains(&ReasonCode::TrustSbomMissing),
        "it is not confused with a failure of the criteria"
    );
    assert!(!decision.applied);
}
