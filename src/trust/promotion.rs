//! The promotion plane: who decides and what is granted.
//!
//! Promoting a digest to the trusted channel is a decision in two halves that
//! are not confused: Cedar says **who** may promote, and the trust
//! policy says whether the **software** deserves it. This piece composes the
//! promotion service with its durable store and with the active
//! policy, and computes **which digests** a promotion covers.
//!
//! # What a promotion covers
//!
//! Promoting an OCI manifest without its config and its layers would leave an
//! image that is advertised and cannot be pulled. That is why the grant covers
//! the requested digest **and** the digests that manifest names; an
//! index also covers the child manifests the repository itself holds,
//! with a bounded depth and number of nodes. A manifest that
//! cannot be read grants only its own digest: a half chain is not
//! widened by conjecture.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use hodei_evidence::ContentDigest;
use hodei_hub::{BlobStore, ReadPolicy};
use hodei_trust_policy::{
    DecisionStore, DecisionStoreError, PolicyRegistry, PromotionDecision, PromotionService,
    PromotionTarget, PromotionTargets, TrustPolicy,
};

/// The distribution promoted to: the trusted channel.
pub const TRUSTED_DISTRIBUTION: &str = "trusted";

/// How deep a manifest index is followed.
const MAX_DEPTH: usize = 4;
/// How many manifests are read at most in a promotion.
const MAX_MANIFESTS: usize = 256;

/// The cap on digests a promotion covers: a manifest with many
/// descriptors does not multiply the catalog queries. Past the cap
/// less is granted, never more.
const MAX_COVERED: usize = 4096;

/// The promotion service and the policy it decides with.
pub struct TrustPlane {
    service: PromotionService,
    registry: PolicyRegistry,
    max_validity: Duration,
    policy_id: String,
}

impl TrustPlane {
    /// A plane over a decision store and a policy registry,
    /// with the validity ceiling every new grant must respect.
    pub fn new(
        store: Arc<dyn DecisionStore>,
        registry: PolicyRegistry,
        policy_id: impl Into<String>,
        max_validity: Duration,
    ) -> Self {
        let policy_id = policy_id.into();
        let service = PromotionService::new(
            store,
            Arc::new(TrustedChannelTargets),
            Arc::new(registry.clone()),
        );
        Self {
            service,
            registry,
            max_validity,
            policy_id,
        }
    }

    /// The service that decides and writes the decision.
    pub fn service(&self) -> &PromotionService {
        &self.service
    }

    /// The identifier of the policy it decides with.
    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }

    /// The plane's policy registry.
    ///
    /// It is one more handle over the SAME inner state: the reactive
    /// cycle (HSP-137) needs a second `PromotionService` --the
    /// first is not cloneable-- that decides against the policy that is
    /// active right now, exactly the one the public promotion uses. Cloning
    /// the registry shares the interior: a policy activated later
    /// is seen from both halves.
    pub fn registry(&self) -> PolicyRegistry {
        self.registry.clone()
    }

    /// The version of the policy that decides **now**.
    ///
    /// It is the second half of the promotion's compare-and-set: the
    /// decision carries the version it was measured with, and a grant
    /// is only published if that version is still the active one. Without an
    /// active policy there is no version to compare against: nothing is published.
    pub fn active_policy_version(&self) -> Option<String> {
        self.registry
            .active(&self.policy_id)
            .map(|policy| policy.version)
    }

    /// The validity of a grant issued by the given decision: the
    /// first required expiry, cut by the plane's ceiling.
    pub fn validity_of(&self, decision: &PromotionDecision, now: DateTime<Utc>) -> DateTime<Utc> {
        grant_valid_until(
            &decision.trust,
            &decision.context.exceptions,
            now,
            self.max_validity,
        )
    }
}

/// The moment at which the grant stops being trustworthy.
///
/// It is the **first expiry** of what the decision required: the minimum of
/// the expiries of the exceptions the decision actually applied
/// (`requirements` with `exception`), always cut by the revalidation
/// ceiling. The policy and the evidence have no date of their own in the
/// registry --the policy changes by version and the evidence by the
/// risk that reevaluates it--, and that is why the ceiling is their common expiry:
/// once the hour passes, the grant goes through a new decision again.
///
/// It is computed over the **persisted** decision, together with its context:
/// what the grant promises is what that decision said, not what the
/// client asked for.
pub fn grant_valid_until(
    trust: &hodei_trust_policy::TrustDecision,
    exceptions: &[hodei_trust_policy::ExceptionFact],
    now: DateTime<Utc>,
    max_validity: Duration,
) -> DateTime<Utc> {
    let ceiling = now + max_validity;
    let exception_limit = trust
        .requirements
        .iter()
        .filter_map(|requirement| requirement.exception.as_ref())
        .filter_map(|id| {
            exceptions
                .iter()
                .find(|exception| &exception.id == id)
                .map(|exception| exception.expires_at)
        })
        .min();
    match exception_limit {
        Some(until) if until < ceiling => until,
        _ => ceiling,
    }
}

/// The destination of every promotion: the account's trusted channel.
pub struct TrustedChannelTargets;

#[async_trait]
impl PromotionTargets for TrustedChannelTargets {
    async fn target_of(
        &self,
        account: &str,
        _subject: &str,
    ) -> Result<Option<PromotionTarget>, DecisionStoreError> {
        Ok(Some(PromotionTarget {
            distribution: TRUSTED_DISTRIBUTION.to_string(),
            account: account.to_string(),
        }))
    }
}

/// A registry with the policy of a JSON file activated, or empty.
///
/// An empty registry is not a registry that permits: every promotion ends up
/// rejected with a written `PolicyUnavailable` decision. A file that
/// is declared and cannot be read or understood **is a startup error**,
/// not an absent policy nobody has paid attention to.
pub fn registry_from(policy_file: Option<&str>, policy_id: &str) -> Result<PolicyRegistry, String> {
    let registry = PolicyRegistry::empty();
    let Some(path) = policy_file else {
        return Ok(registry);
    };
    let text = std::fs::read_to_string(Path::new(path))
        .map_err(|error| format!("the trust policy {path:?} cannot be read: {error}"))?;
    let mut policy: TrustPolicy = serde_json::from_str(&text)
        .map_err(|error| format!("the trust policy {path:?} is not a valid policy: {error}"))?;
    policy.id = policy_id.to_string();
    registry.activate(
        policy,
        "bootstrap",
        "loaded from the trust policy file at the boot",
        chrono::Utc::now(),
    );
    Ok(registry)
}

/// What was read from a manifest, uninterpreted.
enum Read {
    /// The digests it names.
    Edges(Vec<ContentDigest>),
    /// It could not be read or understood.
    Opaque,
}

async fn read_edges(content: &dyn BlobStore, digest: &ContentDigest) -> Read {
    match content.get_with(digest, ReadPolicy::Verify).await {
        Ok(bytes) => match crate::gc::plan::manifest_edges(&bytes) {
            Ok(edges) => Read::Edges(edges),
            Err(_) => Read::Opaque,
        },
        Err(_) => Read::Opaque,
    }
}

/// The digests that promoting `digest`, which lives at `asset_path`, covers.
///
/// `is_manifest_of` says whether a digest is a manifest **of this
/// repository** (it has its `manifests/<hex>` asset and points to that digest);
/// it is the only thing followed from an index. The first in the list is
/// always the requested digest.
pub async fn digests_covered<F, Fut>(
    content: &dyn BlobStore,
    digest: &ContentDigest,
    asset_path: &str,
    is_manifest_of: F,
) -> Vec<ContentDigest>
where
    F: Fn(ContentDigest) -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let mut covered = vec![digest.clone()];
    if !asset_path.starts_with("manifests/") {
        return covered;
    }
    let mut seen: HashSet<ContentDigest> = HashSet::from([digest.clone()]);
    let mut queue: VecDeque<(ContentDigest, usize)> = VecDeque::from([(digest.clone(), 0)]);
    let mut read = 0usize;
    while let Some((current, depth)) = queue.pop_front() {
        if read >= MAX_MANIFESTS {
            break;
        }
        read += 1;
        let Read::Edges(edges) = read_edges(content, &current).await else {
            continue;
        };
        for edge in edges {
            if covered.len() >= MAX_COVERED {
                return covered;
            }
            if !seen.insert(edge.clone()) {
                continue;
            }
            covered.push(edge.clone());
            if depth + 1 < MAX_DEPTH && is_manifest_of(edge.clone()).await {
                queue.push_back((edge, depth + 1));
            }
        }
    }
    covered
}

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_hub::FileBlobStore;

    async fn put(store: &FileBlobStore, bytes: &[u8]) -> ContentDigest {
        store.put(bytes).await.expect("the content is written")
    }

    fn manifest(config: &ContentDigest, layers: &[&ContentDigest]) -> Vec<u8> {
        let layers: Vec<_> = layers
            .iter()
            .map(|layer| serde_json::json!({ "digest": format!("sha256:{}", layer.value) }))
            .collect();
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "config": { "digest": format!("sha256:{}", config.value) },
            "layers": layers,
        }))
        .expect("the manifest serializes")
    }

    #[tokio::test]
    async fn a_blob_covers_only_itself() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let blob = put(&store, b"layer").await;
        let covered = digests_covered(&store, &blob, &format!("blobs/{}", blob.value), |_| async {
            false
        })
        .await;
        assert_eq!(covered, vec![blob]);
    }

    #[tokio::test]
    async fn a_manifest_covers_its_config_and_its_layers() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let config = put(&store, b"config").await;
        let layer = put(&store, b"layer").await;
        let image = put(&store, &manifest(&config, &[&layer])).await;
        let covered = digests_covered(
            &store,
            &image,
            &format!("manifests/{}", image.value),
            |_| async { false },
        )
        .await;
        assert_eq!(covered[0], image);
        assert!(covered.contains(&config));
        assert!(covered.contains(&layer));
        assert_eq!(covered.len(), 3);
    }

    #[tokio::test]
    async fn an_unreadable_manifest_covers_only_itself() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let not_a_manifest = put(&store, b"just bytes").await;
        let covered = digests_covered(
            &store,
            &not_a_manifest,
            &format!("manifests/{}", not_a_manifest.value),
            |_| async { false },
        )
        .await;
        assert_eq!(covered, vec![not_a_manifest]);
    }

    #[tokio::test]
    async fn an_index_covers_the_children_the_repository_has_and_what_they_name() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let config = put(&store, b"config").await;
        let layer = put(&store, b"layer").await;
        let child = put(&store, &manifest(&config, &[&layer])).await;
        let index_bytes = serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "manifests": [{ "digest": format!("sha256:{}", child.value) }],
        }))
        .expect("the index serializes");
        let index = put(&store, &index_bytes).await;
        let known = child.clone();
        let covered = digests_covered(
            &store,
            &index,
            &format!("manifests/{}", index.value),
            move |digest| {
                let known = known.clone();
                async move { digest == known }
            },
        )
        .await;
        for expected in [&index, &child, &config, &layer] {
            assert!(covered.contains(expected), "{expected:?}");
        }
    }

    #[tokio::test]
    async fn a_child_the_repository_does_not_have_is_not_followed() {
        let dir = tempfile::tempdir().expect("a directory");
        let store = FileBlobStore::new(dir.path()).expect("a store");
        let config = put(&store, b"config").await;
        let child = put(&store, &manifest(&config, &[])).await;
        let index_bytes = serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 2,
            "manifests": [{ "digest": format!("sha256:{}", child.value) }],
        }))
        .expect("the index serializes");
        let index = put(&store, &index_bytes).await;
        let covered = digests_covered(
            &store,
            &index,
            &format!("manifests/{}", index.value),
            |_| async { false },
        )
        .await;
        assert!(covered.contains(&child));
        assert!(!covered.contains(&config), "the child was not read");
    }

    #[test]
    fn a_missing_policy_file_is_a_boot_error_and_no_file_is_an_empty_registry() {
        assert!(registry_from(None, "default").is_ok());
        assert!(registry_from(Some("/nonexistent/policy.json"), "default").is_err());
    }

    use hodei_trust_policy::{ExceptionFact, Outcome, RequirementOutcome, TrustDecision};

    fn a_decision(requirements: Vec<RequirementOutcome>) -> TrustDecision {
        TrustDecision {
            outcome: Outcome::Allow,
            reasons: Vec::new(),
            requirements,
            context_version: "1".to_string(),
            policy_id: "default".to_string(),
            policy_version: "1.0.0".to_string(),
            schema_version: "1".to_string(),
            capsule_hash: Some("sha256:cap".to_string()),
            evidence_ids: Vec::new(),
            risk_version: 0,
            decided_at: Utc::now(),
        }
    }

    fn waived_by(exception: &str) -> RequirementOutcome {
        RequirementOutcome {
            requirement: "signed".to_string(),
            satisfied: true,
            reason: None,
            exception: Some(exception.to_string()),
        }
    }

    fn an_exception(id: &str, expires_at: DateTime<Utc>) -> ExceptionFact {
        ExceptionFact {
            id: id.to_string(),
            name: format!("ex-{id}"),
            expires_at,
            applies_to_subject: true,
        }
    }

    /// AC2: the validity is governed by the first expiry of the exceptions that
    /// the decision actually applied, ahead of the ceiling.
    #[test]
    fn the_validity_is_the_earliest_expiry_of_the_exceptions_applied() {
        let now = Utc::now();
        let decision = a_decision(vec![waived_by("ex-a"), waived_by("ex-b")]);
        let exceptions = [
            an_exception("ex-b", now + Duration::days(2)),
            an_exception("ex-a", now + Duration::hours(3)),
        ];
        let until = grant_valid_until(&decision, &exceptions, now, Duration::days(30));
        assert_eq!(until, now + Duration::hours(3));
    }

    /// The revalidation ceiling cuts ahead of everything: without required
    /// exceptions --or with distant exceptions-- the grant lives until the
    /// ceiling, and not one second more.
    #[test]
    fn the_ceiling_bounds_the_validity_when_nothing_expires_before() {
        let now = Utc::now();
        let ceiling = Duration::hours(12);

        let unbounded = grant_valid_until(&a_decision(Vec::new()), &[], now, ceiling);
        assert_eq!(unbounded, now + ceiling);

        let decision = a_decision(vec![waived_by("ex-a")]);
        let far = [an_exception("ex-a", now + Duration::days(90))];
        let bounded = grant_valid_until(&decision, &far, now, ceiling);
        assert_eq!(bounded, now + ceiling);
    }

    /// An exception the decision did not name expires nothing: the validity
    /// is computed from what was required, not from whatever happens to be around.
    #[test]
    fn an_exception_the_decision_did_not_apply_bounds_nothing() {
        let now = Utc::now();
        let decision = a_decision(vec![waived_by("ex-used")]);
        let exceptions = [an_exception("ex-other", now + Duration::minutes(1))];
        let until = grant_valid_until(&decision, &exceptions, now, Duration::hours(6));
        assert_eq!(until, now + Duration::hours(6));
    }

    /// The second half of AC3's compare-and-set: the plane says the
    /// version active **now**, not the one it knew at birth. If the
    /// policy is swapped in the middle of a promotion, the handler's
    /// comparison sees the new one and the decision measured with the old one is not
    /// published.
    #[test]
    fn the_active_version_is_the_live_state_of_the_registry() {
        struct NoDecisions;

        #[async_trait]
        impl DecisionStore for NoDecisions {
            async fn put(&self, _decision: &PromotionDecision) -> Result<(), DecisionStoreError> {
                Ok(())
            }
            async fn get(
                &self,
                _id: &hodei_trust_policy::DecisionId,
            ) -> Result<Option<PromotionDecision>, DecisionStoreError> {
                Ok(None)
            }
            async fn list_for_subject(
                &self,
                _account: &str,
                _subject: &str,
            ) -> Result<Vec<PromotionDecision>, DecisionStoreError> {
                Ok(Vec::new())
            }
            async fn find_by_correlation(
                &self,
                _correlation_id: &str,
            ) -> Result<Option<PromotionDecision>, DecisionStoreError> {
                Ok(None)
            }
        }

        let registry = PolicyRegistry::empty();
        registry.activate(
            TrustPolicy::new("default", "1", Vec::new()),
            "operator",
            "first",
            Utc::now(),
        );
        let plane = TrustPlane::new(
            Arc::new(NoDecisions),
            registry.clone(),
            "default",
            Duration::hours(1),
        );
        assert_eq!(plane.active_policy_version().as_deref(), Some("1"));

        registry.activate(
            TrustPolicy::new("default", "2", Vec::new()),
            "operator",
            "swap",
            Utc::now(),
        );
        assert_eq!(
            plane.active_policy_version().as_deref(),
            Some("2"),
            "a decision measured with 1 is stale the moment 2 is active"
        );
    }
}
