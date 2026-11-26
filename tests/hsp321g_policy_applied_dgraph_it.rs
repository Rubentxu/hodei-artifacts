//! HSP-321 (tramo g): activating or rolling back in the Policy Studio changes
//! the policy the promotion plane REALLY applies, per account, with no restart.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every Dgraph IT (without it the
//! tests SKIP). Run with `--test-threads=1`. The plane under test is the
//! production one (`TrustPlane::per_account`) over the real
//! `DgraphLifecycleStore` and `DgraphDecisionStore`; the decision is the one
//! `PromotionService::request_promotion` writes. What it proves:
//!
//! - an account without a head fails closed (`PolicyUnavailable`);
//! - activating a revision that denies flips the very next decision from
//!   allow to deny, and a rollback restores allow, with no restart;
//! - one account's activation never changes another account's decision;
//! - a brand-new plane over brand-new connections (a restart) applies the
//!   durable head with no boot step.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use hodei_artifacts_api::trust::promotion::TrustPlane;
use hodei_evidence::ContentDigest;
use hodei_evidence::domain::capsule::{
    Dimension, DimensionName, DimensionState, TrustCapsule, TrustProfile,
};
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_trust_policy::lifecycle::{Actors, SeparationPolicy};
use hodei_trust_policy::reasons::ReasonCode;
use hodei_trust_policy::{
    AuthorizationResult, DecisionId, DecisionStore, LifecycleStore, Outcome, PolicyRegistry,
    PromotionRequest, Requirement, RequirementKind, TrustContext, TrustPolicy,
};
use hodei_trust_policy_storage::{DgraphDecisionStore, DgraphLifecycleStore};
use kernel::Hrn;

const POLICY_ID: &str = "production";

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let mut text =
        format!("endpoint = \"{endpoint}\"\n\n[tls]\nenabled = false\nca_cert_path = \"\"\n");
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

async fn fresh_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut last = String::new();
    for attempt in 1..=3 {
        match GraphClient::connect(config.clone()).await {
            Ok(mut client) => match client.login().await {
                Ok(()) => return Some(Arc::new(client)),
                Err(error) => last = error.to_string(),
            },
            Err(error) => last = error.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(250 * attempt)).await;
    }
    panic!("Dgraph did not open after 3 attempts: {last}");
}

fn run_scope(tag: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock moves")
        .as_nanos() as u64;
    format!("hsp321g-{tag}-{nanos}")
}

/// A plane over a fresh pair of connections, as a restarted service has.
async fn plane_and_stores(
    seed: PolicyRegistry,
) -> Option<(
    TrustPlane,
    Arc<DgraphDecisionStore>,
    Arc<dyn LifecycleStore>,
)> {
    let client = fresh_client().await?;
    let decisions = Arc::new(
        DgraphDecisionStore::open(client.clone())
            .await
            .expect("the decision store opens"),
    );
    let lifecycle: Arc<dyn LifecycleStore> = Arc::new(
        DgraphLifecycleStore::open(client, SeparationPolicy::required())
            .await
            .expect("the lifecycle opens"),
    );
    let plane = TrustPlane::per_account(
        decisions.clone(),
        lifecycle.clone(),
        seed,
        POLICY_ID,
        chrono::Duration::hours(1),
    );
    Some((plane, decisions, lifecycle))
}

fn subject(account: &str) -> Hrn {
    Hrn::from_string(&format!(
        "hrn:hodei:artifacts:eu-west-1:{account}:Artifact/web-1.0.0"
    ))
    .expect("an HRN")
}

/// A capsule whose integrity is verified and whose inventory was never taken.
fn capsule(account: &str) -> TrustCapsule {
    TrustCapsule {
        subject: subject(account),
        profile: TrustProfile {
            subject: subject(account),
            dimensions: vec![Dimension {
                name: DimensionName::Integrity,
                state: DimensionState::Verified,
                evidence: Vec::new(),
                reason: None,
            }],
        },
        version: "1.0.0".to_string(),
        content_hash: ContentDigest::of_bytes(b"hsp321g-capsule"),
        assembled_at: Utc::now(),
    }
}

/// Version 1 asks only for integrity (the capsule passes); the version that
/// denies also demands a complete inventory (the capsule has none: the gate answers INDETERMINATE for an unmeasured requirement, never ALLOW).
fn policy(version: &str, demands_inventory: bool) -> TrustPolicy {
    let mut requirements = vec![Requirement::denying(RequirementKind::Dimension {
        dimension: DimensionName::Integrity,
        accepts: vec![DimensionState::Verified],
    })];
    if demands_inventory {
        requirements.push(Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::InventoryCompleteness,
            accepts: vec![DimensionState::Complete],
        }));
    }
    TrustPolicy::new(POLICY_ID, version, requirements)
}

/// Saves by the editor and activates by the approver (separation required).
async fn save_and_activate(
    lifecycle: &Arc<dyn LifecycleStore>,
    account: &str,
    policy: TrustPolicy,
    expected_head: Option<u64>,
) -> u64 {
    let revision = lifecycle
        .save(account, policy, "editor", "it", Utc::now())
        .await
        .expect("the revision is saved");
    lifecycle
        .activate(
            account,
            POLICY_ID,
            revision.sequence,
            expected_head,
            "approver",
            "it",
            Utc::now(),
        )
        .await
        .expect("the revision is activated")
        .sequence
}

/// One real promotion decision, read back from the durable ledger.
async fn decide(
    plane: &TrustPlane,
    decisions: &DgraphDecisionStore,
    account: &str,
    n: usize,
) -> (Outcome, Vec<ReasonCode>) {
    let id = DecisionId::new(format!("{account}-dec-{n}"));
    let request = PromotionRequest::new(
        format!("{account}-corr-{n}"),
        "hrn:hodei:iam::acme:User/alice",
        account,
        subject(account).to_string(),
        "trusted",
        Utc::now(),
    );
    let context = TrustContext::builder(&capsule(account)).build();
    plane
        .service()
        .request_promotion(
            id.clone(),
            request,
            AuthorizationResult::Permit,
            context,
            plane.policy_id(),
        )
        .await
        .expect("the promotion is decided");
    let decision = decisions
        .get(&id)
        .await
        .expect("the ledger reads")
        .expect("the decision was written");
    (decision.outcome, decision.reasons)
}

#[tokio::test]
async fn the_activated_head_is_the_policy_that_decides_per_account() {
    let Some((plane, decisions, lifecycle)) = plane_and_stores(PolicyRegistry::empty()).await
    else {
        return;
    };
    let a = run_scope("a");
    let b = run_scope("b");

    // No head and no seed: fail closed, never an empty policy that permits.
    let (outcome, reasons) = decide(&plane, &decisions, &a, 1).await;
    assert_ne!(outcome, Outcome::Allow);
    assert!(
        reasons.contains(&ReasonCode::PolicyUnavailable),
        "{reasons:?}"
    );
    assert_eq!(plane.active_policy_version_for(&a).await, None);

    // Activating v1 makes the very next decision allow, with no restart.
    let head1 = save_and_activate(&lifecycle, &a, policy("1", false), None).await;
    assert_eq!(
        plane.active_policy_version_for(&a).await.as_deref(),
        Some("1")
    );
    let (outcome, _) = decide(&plane, &decisions, &a, 2).await;
    assert_eq!(outcome, Outcome::Allow, "the activated v1 permits");

    // Activating a revision that denies flips the next decision to deny.
    save_and_activate(&lifecycle, &a, policy("2", true), Some(head1)).await;
    assert_eq!(
        plane.active_policy_version_for(&a).await.as_deref(),
        Some("2")
    );
    let (outcome, _) = decide(&plane, &decisions, &a, 3).await;
    assert_eq!(
        outcome,
        Outcome::Indeterminate,
        "the activated v2 refuses (no inventory)"
    );

    // Another account never sees A's activation.
    let (outcome, reasons) = decide(&plane, &decisions, &b, 1).await;
    assert_ne!(outcome, Outcome::Allow);
    assert!(
        reasons.contains(&ReasonCode::PolicyUnavailable),
        "{reasons:?}"
    );
    let head_b = save_and_activate(&lifecycle, &b, policy("1", false), None).await;
    let (outcome, _) = decide(&plane, &decisions, &b, 2).await;
    assert_eq!(outcome, Outcome::Allow, "B activated v1 and permits");
    let (outcome, _) = decide(&plane, &decisions, &a, 4).await;
    assert_eq!(
        outcome,
        Outcome::Indeterminate,
        "B's activation left A on v2"
    );

    // Rollback to revision 1 restores allow for A, immediately.
    let head = lifecycle
        .head(&a, POLICY_ID)
        .await
        .expect("the head reads")
        .expect("A has a head");
    lifecycle
        .rollback(
            &a,
            POLICY_ID,
            1,
            Some(head.sequence),
            &Actors::new("editor", "approver"),
            "it",
            Utc::now(),
        )
        .await
        .expect("the rollback applies");
    let (outcome, _) = decide(&plane, &decisions, &a, 5).await;
    assert_eq!(outcome, Outcome::Allow, "the rollback restored the permit");

    // A restart: a brand-new plane over brand-new connections applies the
    // durable head, with no boot step.
    let Some((restarted, decisions2, _)) = plane_and_stores(PolicyRegistry::empty()).await else {
        return;
    };
    let (outcome, _) = decide(&restarted, &decisions2, &a, 6).await;
    assert_eq!(
        outcome,
        Outcome::Allow,
        "the restarted service applies the head"
    );
    save_and_activate(&lifecycle, &b, policy("2", true), Some(head_b)).await;
    let (outcome, _) = decide(&restarted, &decisions2, &b, 3).await;
    assert_eq!(
        outcome,
        Outcome::Indeterminate,
        "and sees B's later head too"
    );
}

/// The explicit seed answers only for an account that never activated.
#[tokio::test]
async fn the_declared_seed_serves_only_accounts_without_a_head() {
    let seed = PolicyRegistry::empty();
    seed.activate(policy("seed", false), "bootstrap", "seed", Utc::now());
    let Some((plane, decisions, lifecycle)) = plane_and_stores(seed).await else {
        return;
    };
    let a = run_scope("seeded");
    let (outcome, _) = decide(&plane, &decisions, &a, 1).await;
    assert_eq!(
        outcome,
        Outcome::Allow,
        "the seed answers for a headless account"
    );
    assert_eq!(
        plane.active_policy_version_for(&a).await.as_deref(),
        Some("seed")
    );

    save_and_activate(&lifecycle, &a, policy("2", true), None).await;
    let (outcome, _) = decide(&plane, &decisions, &a, 2).await;
    assert_eq!(
        outcome,
        Outcome::Indeterminate,
        "a head always wins over the seed"
    );
}
