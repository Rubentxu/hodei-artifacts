//! HSP-154 / AC3: rotating a trust root records the revision and
//! reproceses the grants the old revision vouched for, through the
//! reevaluation chain that already exists (HAE-04).
//!
//! The AC has two halves and the test asserts both:
//!
//! 1. The rotation is a **bookkeeping fact**: the lineage moved one
//!    revision forward, the move was recorded with who and why, and the
//!    old revision was revoked by the rotation itself.
//! 2. The reproceso runs on the **existing** chain -- `Reevaluation` with
//!    the new `TrustRootRotated` reason through `PromotionDecisionRequester`
//!    and `PromotionService`. The reevaluation that refuses writes a NEW
//!    decision that names the one it supersedes, and the grants of the
//!    superseded decision are taken back through the ledger. It does NOT
//!    rewrite the historical decision: the store is append-only, and the
//!    test compares the stored bytes of the old decision before and after.
//!
//! Everything runs in-memory: Surreal `Mem` for the decision store, a
//! recording double for the ledger. No Dgraph, no network.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_catalog::{CatalogError, TrustGrant, TrustLedger};
use hodei_evidence::domain::capsule::{DimensionName, DimensionState};
use hodei_evidence::standards::dsse::VerificationKey;
use hodei_evidence::standards::{RootRevision, TrustRoots};
use hodei_reactive::reactions::{
    DecisionRequester, ReasonCode as ReactiveReasonCode, Reevaluation,
};
use hodei_trust_policy::context::{
    DimensionFact, Exposure, RiskFacts, SubjectFact, TargetEnvironment, TRUST_CONTEXT_VERSION,
};
use hodei_trust_policy::gate::AuthorizationResult;
use hodei_trust_policy::ports::{DecisionStore, PromotionTargets};
use hodei_trust_policy::promotion::{DecisionId, PromotionRequest, PromotionTarget};
use hodei_trust_policy::service::PromotionService;
use hodei_trust_policy::{
    Outcome, PolicyRegistry, PromotionOutcome, Requirement, RequirementKind, TrustContext,
    TrustPolicy,
};
use hodei_trust_policy_storage::reactions::{PromotionDecisionRequester, TrustContextSource};
use hodei_trust_policy_storage::{StaticPromotionTargets, SurrealDecisionStore};
use surrealdb::engine::local::{Db, Mem};
use surrealdb::Surreal;

const ACCOUNT: &str = "acme";
const SUBJECT: &str = "hrn:hodei:artifacts:eu-west-1:acme:Artifact/app-1.0.0";

fn at(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("a timestamp")
        .with_timezone(&Utc)
}

/// The policy of production, the one the promotion contract uses: the
/// integrity dimension has to say `Verified` and the risk be established.
fn production_policy() -> TrustPolicy {
    TrustPolicy::new(
        "production",
        "1.0.0",
        vec![
            Requirement::denying(RequirementKind::Dimension {
                dimension: DimensionName::Integrity,
                accepts: vec![DimensionState::Verified],
            }),
            Requirement::denying(RequirementKind::RiskEstablished),
        ],
    )
}

/// The context a rotation reevaluation answers with: the integrity
/// dimension as the evidence side saw it, and the risk established.
fn context_for(dimensions: Vec<DimensionFact>, assembled_at: DateTime<Utc>) -> TrustContext {
    TrustContext {
        version: TRUST_CONTEXT_VERSION.to_string(),
        subject: SubjectFact {
            hrn: SUBJECT.to_string(),
            digest: None,
        },
        target: TargetEnvironment::Production,
        dimensions,
        capsule_hash: None,
        risk: RiskFacts {
            established: true,
            exposure: Exposure::Internal,
            ..RiskFacts::default()
        },
        exceptions: Vec::new(),
        evidence_ids: Vec::new(),
        assembled_at,
    }
}

/// A revocation record from the evidence-side bookkeeping, so the test
/// reads like the rotation that would have triggered this reevaluation.
fn revision(key_id: &str, revision: u32, revoked_at: Option<DateTime<Utc>>) -> RootRevision {
    RootRevision {
        lineage: "alpha".to_string(),
        revision,
        key: VerificationKey {
            key_id: key_id.to_string(),
            point: vec![0x04; 65],
        },
        issuer: Some("registry.example".to_string()),
        activated_at: at("2026-01-01T00:00:00Z"),
        retired_at: None,
        revoked_at,
    }
}

/// The ledger double of the promotion contract: it does not hold grants,
/// it records the revocations the reevaluation asked for.
#[derive(Default)]
struct RecordingLedger {
    revoked: Mutex<Vec<(String, String, String)>>,
}

#[async_trait]
impl TrustLedger for RecordingLedger {
    async fn grant(&self, _grants: &[TrustGrant]) -> Result<(), CatalogError> {
        Ok(())
    }

    async fn revoke_decision(
        &self,
        _account: &str,
        _repository: &str,
        _decision_id: &str,
        _actor: &str,
        _reason: &str,
        _at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        Ok(0)
    }

    async fn revoke_decision_for_account(
        &self,
        account: &str,
        decision_id: &str,
        actor: &str,
        _reason: &str,
        _at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.revoked
            .lock()
            .expect("the record is not poisoned")
            .push((
                account.to_string(),
                decision_id.to_string(),
                actor.to_string(),
            ));
        Ok(1)
    }

    async fn revoke_digest(
        &self,
        _account: &str,
        _repository: &str,
        _digest: &hodei_evidence::ContentDigest,
        _actor: &str,
        _reason: &str,
        _at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        Ok(0)
    }

    async fn active_grant(
        &self,
        _account: &str,
        _repository: &str,
        _digest: &hodei_evidence::ContentDigest,
        _at: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError> {
        Ok(None)
    }

    async fn grants_of(
        &self,
        _account: &str,
        _repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError> {
        Ok(Vec::new())
    }
}

/// A context source that answers the reevaluation with the dimensions it
/// carries: how the evidence side of a rotation -- or of a root that no
/// longer vouches for the release -- reaches the policy.
struct FixedDimensions {
    dimensions: Vec<DimensionFact>,
}

#[async_trait]
impl TrustContextSource for FixedDimensions {
    async fn context_of(
        &self,
        _subject: &str,
        _critical_reachable: u32,
        at: DateTime<Utc>,
    ) -> Result<TrustContext, String> {
        Ok(context_for(self.dimensions.clone(), at))
    }
}

/// The world of the test: store, policy, promotion service.
async fn the_world() -> (Arc<SurrealDecisionStore<Db>>, Arc<PromotionService>) {
    let db = Arc::new(
        Surreal::new::<Mem>(())
            .await
            .expect("the memory engine opens"),
    );
    db.use_ns("hodei")
        .use_db("trust")
        .await
        .expect("the namespace");
    let store = Arc::new(SurrealDecisionStore::new(db));
    let policies = Arc::new(PolicyRegistry::empty());
    policies.activate(
        production_policy(),
        "alice",
        "the policy of production",
        at("2026-01-01T00:00:00Z"),
    );
    let targets: Arc<dyn PromotionTargets> =
        Arc::new(StaticPromotionTargets::new(vec![PromotionTarget {
            distribution: "production".to_string(),
            account: ACCOUNT.to_string(),
        }]));
    let service = Arc::new(PromotionService::new(
        store.clone(),
        targets,
        policies.clone(),
    ));
    (store, service)
}

/// AC3: the rotation moves the lineage one revision forward and records
/// the move, and the reevaluation it triggers reproceses the grants
/// through the existing chain WITHOUT rewriting the historical decision.
#[tokio::test]
async fn a_root_rotation_is_recorded_and_reproceses_grants_without_rewriting_history() {
    // Half one: the rotation, as the evidence side of the platform
    // books it (TrustRoots lives in hodei-evidence; the record is what
    // the rotation of AC3 has to leave behind).
    let mut roots = TrustRoots::new();
    roots.activate(revision("k1", 1, None), "alice", "the shipped root");
    let record = roots
        .rotate(
            "alpha",
            revision("k2", 2, None),
            // A rotation for compromise: the previous revision is
            // revoked, and that is what makes the grants it vouched for
            // have to be reevaluated.
            true,
            at("2026-03-05T00:00:00Z"),
            "secops",
            "k1 leaked",
        )
        .expect("the rotation is the next revision");
    assert_eq!(record.from_revision, Some(1));
    assert_eq!(record.to_revision, 2);
    assert!(record.revoked_previous);
    assert_eq!(roots.history().len(), 2, "the activation and the rotation");
    assert!(roots
        .find_key("k1")
        .expect("k1 stays reachable")
        .is_revoked_at(at("2026-03-05T00:00:00Z")));
    assert_eq!(roots.current("alpha").expect("a current").key.key_id, "k2");
    // The bookkeeping of the rotation does NOT touch decisions: there
    // are none in the roots, and none of the decision machinery ran.
    // Half two is where the decisions are reprocesed, through HAE-04.

    let (store, service) = the_world().await;

    // The decision the old root vouched for: an ALLOW, applied, so its
    // grants are live.
    let prior_id = DecisionId::new("dec-prior");
    let outcome = service
        .request_promotion(
            prior_id.clone(),
            PromotionRequest::new(
                "corr-build-001",
                "alice",
                ACCOUNT.to_string(),
                SUBJECT.to_string(),
                "production".to_string(),
                at("2026-03-04T10:15:00Z"),
            ),
            AuthorizationResult::Permit,
            context_for(vec![verified_integrity()], at("2026-03-04T10:15:00Z")),
            "production",
        )
        .await
        .expect("the promotion is written");
    assert_eq!(
        outcome,
        PromotionOutcome::Promoted,
        "the root was trusted then"
    );
    let prior_stored = store
        .get(&prior_id)
        .await
        .expect("the store answers")
        .expect("the prior decision is there");
    assert!(
        prior_stored.applied,
        "and it had a real effect: its grants are live"
    );

    // A snapshot of the historical decision's bytes: the rotation chain
    // must leave them EXACTLY as they were.
    let prior_bytes = serde_json::to_string(&prior_stored).expect("the decision serializes");

    // The reevaluation the rotation triggers, through the EXISTING
    // HAE-04 chain. The evidence answer for a release whose root was
    // revoked: the integrity dimension no longer says `Verified`.
    let ledger = Arc::new(RecordingLedger::default());
    let requester = Arc::new(
        PromotionDecisionRequester::new(
            service.clone(),
            store.clone(),
            Arc::new(FixedDimensions {
                dimensions: vec![DimensionFact {
                    name: DimensionName::Integrity,
                    state: DimensionState::Invalid,
                }],
            }),
            "production",
            "production",
            "system",
        )
        .with_ledger(ledger.clone()),
    );

    let new_id = requester
        .request_reevaluation(Reevaluation::new(
            SUBJECT,
            ReactiveReasonCode::TrustRootRotated,
            at("2026-03-05T00:00:00Z"),
        ))
        .await
        .expect("the reevaluation produces a decision");

    // The reevaluation decided with the reason of the rotation: the
    // identity of the new decision carries the label the reason maps to.
    assert!(
        new_id.starts_with("dec-root-rotation:"),
        "the new decision says WHY it exists: {new_id}"
    );

    // The new decision names the one it supersedes, and it is a DENY:
    // the truth the rotation created does not support the old allow.
    let decisions = store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .expect("the decisions are read");
    assert_eq!(decisions.len(), 2, "the store kept both: append-only");
    let newest = decisions
        .iter()
        .find(|decision| decision.id.as_str() == new_id)
        .expect("the new decision is stored");
    assert_eq!(
        newest.request.correlation_id,
        prior_stored.id.as_str(),
        "the new decision names the one it supersedes"
    );
    assert!(
        !newest.applied && newest.outcome == Outcome::Deny,
        "the reevaluation refused under the rotated root: {:?}",
        newest.outcome
    );

    // The grants the superseded decision conceded were taken back,
    // account-wide, through the EXISTING invalidation of trust (HAE-04).
    let revoked = ledger
        .revoked
        .lock()
        .expect("the record is not poisoned")
        .clone();
    assert_eq!(
        revoked,
        vec![(
            ACCOUNT.to_string(),
            "dec-prior".to_string(),
            "reevaluation:root-rotation".to_string(),
        )],
        "the refused decision's grants were revoked, and only those"
    );

    // And the historical decision was NOT rewritten: same id, same
    // bytes, still readable as it was. The platform's answer to a bad
    // root is a new decision and revoked grants -- not editorial history.
    let prior_after = store
        .get(&prior_id)
        .await
        .expect("the store answers")
        .expect("the historical decision is still there");
    assert_eq!(
        serde_json::to_string(&prior_after).expect("the decision serializes"),
        prior_bytes,
        "the historical decision is untouched, byte for byte"
    );
    assert_eq!(prior_after.outcome, Outcome::Allow);
    assert!(
        prior_after.applied,
        "what it said when it was taken, stays said"
    );
}

/// The rotation that does not revoke has no grants to reproceses, and
/// the chain says so: a reevaluation that still allows writes its
/// decision and leaves the ledger untouched. (The negative case of
/// AC3: rotation is not revocation.)
#[tokio::test]
async fn a_rotation_without_revocation_reevaluates_to_an_allow_and_revokes_nothing() {
    let (store, service) = the_world().await;

    let prior_id = DecisionId::new("dec-prior-clean");
    service
        .request_promotion(
            prior_id.clone(),
            PromotionRequest::new(
                "corr-build-002",
                "alice",
                ACCOUNT.to_string(),
                SUBJECT.to_string(),
                "production".to_string(),
                at("2026-03-04T10:15:00Z"),
            ),
            AuthorizationResult::Permit,
            context_for(vec![verified_integrity()], at("2026-03-04T10:15:00Z")),
            "production",
        )
        .await
        .expect("the promotion is written");

    let ledger = Arc::new(RecordingLedger::default());
    let requester = Arc::new(
        PromotionDecisionRequester::new(
            service.clone(),
            store.clone(),
            // The rotation ran and the new root vouches for the release:
            // the evidence answer is a verified integrity dimension.
            Arc::new(FixedDimensions {
                dimensions: vec![verified_integrity()],
            }),
            "production",
            "production",
            "system",
        )
        .with_ledger(ledger.clone()),
    );

    let new_id = requester
        .request_reevaluation(Reevaluation::new(
            SUBJECT,
            ReactiveReasonCode::TrustRootRotated,
            at("2026-03-05T00:00:00Z"),
        ))
        .await
        .expect("the reevaluation produces a decision");

    let decisions = store
        .list_for_subject(ACCOUNT, SUBJECT)
        .await
        .expect("read");
    assert_eq!(
        decisions.len(),
        2,
        "the clean rotation also writes its decision"
    );
    let newest = decisions
        .iter()
        .find(|decision| decision.id.as_str() == new_id)
        .expect("the new decision");
    assert_eq!(newest.outcome, Outcome::Allow);
    assert!(newest.applied);
    assert!(
        ledger.revoked.lock().expect("clean").is_empty(),
        "a rotation that revoked nothing has no grants to take back"
    );
}

/// The integrity dimension as the release looked while its root was
/// trusted.
fn verified_integrity() -> DimensionFact {
    DimensionFact {
        name: DimensionName::Integrity,
        state: DimensionState::Verified,
    }
}
