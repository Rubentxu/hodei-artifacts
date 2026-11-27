//! HSP-320 tramo 1 (origin HSP-146 / HAE-13): trust invalidation between
//! two nodes that share one Dgraph as the single source of truth.
//!
//! The only per-process state that can serve a revoked trust is the
//! admission cache (`CachedTrustLedger`). Each "node" here is an
//! independent assembly of what a real process builds in `profile.rs`: its
//! own `GraphClient` connection, its own `DgraphTrustStore` and its own
//! cache. They share nothing but the Dgraph.
//!
//! What it proves against the real engine:
//!
//! - **Bound**: node A grants, node B serves it (from cache: no ledger
//!   read); A revokes; B stops honoring it within `verify` + a probe round
//!   trip, with a cache `ttl` of 30 s that would otherwise have kept the
//!   revoked trust alive. The measured propagation is asserted and printed.
//! - **Strict profile** (`verify = 0`): the very next request on B denies.
//! - **In flight, duplicate and replay**: B reads continuously while A
//!   revokes twice (the repeat confirms nothing and does not move the
//!   watermark); once B denies, it never serves the revoked trust again.
//! - **Fail closed**: B partitioned from the authority (the real Dgraph
//!   stays up for A; B's ledger is cut by a switch) never renews a cached
//!   ALLOW past the lease, and after healing it does not resurrect it.
//! - **Normal path**: a repeated read on B is a cache hit (no ledger
//!   read), and a re-grant under a new decision is served at once.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack (with
//! HODEI_REQUIRE_IT=1 a missing harness fails instead of skipping), and
//! intended to run with `--test-threads=1`: the watermark is a global count
//! of the graph's revoked events, so deltas are asserted on a quiet graph.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use hodei_artifacts_api::trust::cache::CachedTrustLedger;
use hodei_catalog::{CatalogError, TrustGrant, TrustLedger};
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig, Tenant};
use hodei_trust_policy_storage::DgraphTrustStore;

/// The cache ttl of every node: long on purpose. Without the watermark a
/// foreign revocation would be honored for this long.
const TTL: Duration = Duration::from_secs(30);
/// The lease of the watermark check on the non-strict nodes.
const VERIFY: Duration = Duration::from_millis(1_000);
/// Slack for the probe round trips and scheduling on top of the lease.
const SLACK: Duration = Duration::from_millis(2_000);

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

/// One independent connection, as one process would open it.
async fn open_client() -> Option<Arc<GraphClient>> {
    let Some(config) = config_from_env() else {
        assert!(
            std::env::var("HODEI_REQUIRE_IT").as_deref() != Ok("1"),
            "HODEI_REQUIRE_IT=1 but HODEI_GRAPH_IT_ENDPOINT is not set"
        );
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return None;
    };
    let mut client = GraphClient::connect(config).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    Some(Arc::new(client))
}

fn unique(tag: &str) -> String {
    static RUN: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock moves")
        .as_nanos();
    format!(
        "hsp320a-{tag}-{nanos}-{}",
        RUN.fetch_add(1, Ordering::SeqCst)
    )
}

/// The shared authority as one node sees it: the real Dgraph ledger behind
/// a switch that can cut this node off (a partition seen from this node
/// only) and with counters of what actually reached the authority.
struct NodeLedger {
    inner: DgraphTrustStore,
    partitioned: AtomicBool,
    /// Only the watermark probe fails; the book keeps answering.
    probe_cut: AtomicBool,
    active_reads: AtomicUsize,
    probes: AtomicUsize,
}

impl NodeLedger {
    fn cut(&self) -> Result<(), CatalogError> {
        if self.partitioned.load(Ordering::SeqCst) {
            Err(CatalogError::unavailable("partitioned from the authority"))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl TrustLedger for NodeLedger {
    async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
        self.cut()?;
        self.inner.grant(grants).await
    }

    async fn revoke_decision(
        &self,
        account: &str,
        repository: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.cut()?;
        self.inner
            .revoke_decision(account, repository, decision_id, actor, reason, at)
            .await
    }

    async fn revoke_decision_for_account(
        &self,
        account: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.cut()?;
        self.inner
            .revoke_decision_for_account(account, decision_id, actor, reason, at)
            .await
    }

    async fn revoke_digest(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        self.cut()?;
        self.inner
            .revoke_digest(account, repository, digest, actor, reason, at)
            .await
    }

    async fn active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        at: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError> {
        self.cut()?;
        self.active_reads.fetch_add(1, Ordering::SeqCst);
        self.inner
            .active_grant(account, repository, digest, at)
            .await
    }

    async fn grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError> {
        self.cut()?;
        self.inner.grants_of(account, repository).await
    }

    async fn revocation_watermark(&self) -> Result<Option<u64>, CatalogError> {
        self.cut()?;
        if self.probe_cut.load(Ordering::SeqCst) {
            return Err(CatalogError::unavailable("the probe is lost"));
        }
        self.probes.fetch_add(1, Ordering::SeqCst);
        self.inner.revocation_watermark().await
    }
}

/// One node: its own connection, its own ledger adapter, its own cache.
struct Node {
    ledger: Arc<NodeLedger>,
    cache: CachedTrustLedger,
}

impl Node {
    async fn open(tenant: &Tenant, verify: Duration) -> Option<Self> {
        let client = open_client().await?;
        let store = DgraphTrustStore::open(client, tenant.clone())
            .await
            .expect("the trust ledger opens");
        let ledger = Arc::new(NodeLedger {
            inner: store,
            partitioned: AtomicBool::new(false),
            probe_cut: AtomicBool::new(false),
            active_reads: AtomicUsize::new(0),
            probes: AtomicUsize::new(0),
        });
        let cache = CachedTrustLedger::new(ledger.clone(), TTL).with_verify(verify);
        Some(Self { ledger, cache })
    }

    fn reads(&self) -> usize {
        self.ledger.active_reads.load(Ordering::SeqCst)
    }

    async fn serves(&self, account: &str, repo: &str, digest: &ContentDigest) -> bool {
        self.cache
            .active_grant(account, repo, digest, Utc::now())
            .await
            .expect("the node answers")
            .is_some()
    }
}

fn a_grant(account: &str, repo: &str, digest: &ContentDigest, decision: &str) -> TrustGrant {
    TrustGrant {
        account: account.to_string(),
        repository: repo.to_string(),
        digest: digest.clone(),
        decision_id: decision.to_string(),
        actor: "hrn:hodei:iam:acme:User/alice".to_string(),
        granted_at: Utc::now(),
        revoked_at: None,
        revoked_by: None,
        revoke_reason: None,
        subject: Some("hrn:hodei:artifacts:eu-west-1:acme:Artifact/app".to_string()),
        policy_version: Some("1.0.0".to_string()),
        capsule_hash: Some("sha256:cap".to_string()),
        risk_version: Some(7),
        valid_until: Some(Utc::now() + ChronoDuration::hours(1)),
    }
}

/// The scenario every test starts from: a tenant, an account and a digest
/// granted by node A under a decision, and node B serving it.
struct World {
    account: String,
    repo: String,
    digest: ContentDigest,
    a: Node,
    b: Node,
}

async fn world(tag: &str, b_verify: Duration) -> Option<World> {
    let tenant = Tenant::new(&unique(tag)).expect("the run tenant is valid");
    // A's own lease is irrelevant to the scenario: A is the writer.
    let a = Node::open(&tenant, VERIFY).await?;
    let b = Node::open(&tenant, b_verify).await?;
    Some(World {
        account: unique("acct"),
        repo: "raw".to_string(),
        digest: ContentDigest::of_bytes(unique("bytes").as_bytes()),
        a,
        b,
    })
}

impl World {
    async fn a_grants(&self, decision: &str) {
        self.a
            .cache
            .grant(&[a_grant(&self.account, &self.repo, &self.digest, decision)])
            .await
            .expect("node A grants");
    }

    async fn a_revokes(&self, decision: &str) -> u64 {
        self.a
            .cache
            .revoke_decision(
                &self.account,
                &self.repo,
                decision,
                "alice",
                "cve",
                Utc::now(),
            )
            .await
            .expect("node A revokes")
    }

    async fn b_serves(&self) -> bool {
        self.b.serves(&self.account, &self.repo, &self.digest).await
    }
}

/// AC: A revokes, B stops honoring within `verify` + probe round trip,
/// even though B's cache ttl is 30 s. The propagation is measured.
#[tokio::test]
async fn a_revocation_by_node_a_stops_node_b_within_the_bound() {
    let Some(w) = world("bound", VERIFY).await else {
        return;
    };
    w.a_grants("d1").await;

    // B serves the grant and the second request is a cache hit.
    assert!(w.b_serves().await, "B honors the grant node A wrote");
    let reads = w.b.reads();
    assert!(w.b_serves().await);
    assert_eq!(w.b.reads(), reads, "the repeat is served by B's cache");

    assert_eq!(w.a_revokes("d1").await, 1, "A confirms the revocation");
    let confirmed = Instant::now();

    // B polls like a client retrying; it must deny within the bound.
    let deadline = VERIFY + SLACK;
    let denied_after = loop {
        if !w.b_serves().await {
            break confirmed.elapsed();
        }
        assert!(
            confirmed.elapsed() <= deadline,
            "B still honors a trust revoked {:?} ago (bound {:?}, ttl {:?})",
            confirmed.elapsed(),
            deadline,
            TTL
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    eprintln!("HSP-320a propagation A->B: {denied_after:?} (lease {VERIFY:?})");
    assert!(
        denied_after <= deadline,
        "propagation {denied_after:?} exceeds the bound {deadline:?}"
    );
    // A request after the bound is denied, and stays denied.
    tokio::time::sleep(VERIFY + Duration::from_millis(100)).await;
    assert!(!w.b_serves().await, "a request past the bound is denied");
}

/// Strict profile: with `verify = 0` the very next request denies.
#[tokio::test]
async fn the_strict_profile_denies_on_the_very_next_request() {
    let Some(w) = world("strict", Duration::ZERO).await else {
        return;
    };
    w.a_grants("d1").await;
    assert!(w.b_serves().await);

    assert_eq!(w.a_revokes("d1").await, 1);
    assert!(
        !w.b_serves().await,
        "with verify=0 the next request on B reads the moved watermark"
    );
}

/// In flight, duplicate and replayed revocations: B reads in a loop while
/// A revokes twice; the repeat confirms nothing and moves nothing; once B
/// denies it never serves the revoked trust again.
#[tokio::test]
async fn in_flight_duplicate_and_replayed_revocations_never_reopen_the_trust() {
    let Some(w) = world("inflight", Duration::from_millis(100)).await else {
        return;
    };
    let w = Arc::new(w);
    w.a_grants("d1").await;
    assert!(w.b_serves().await);

    let before = w.a.ledger.revocation_watermark().await.unwrap().unwrap();

    // B's readers run concurrently with the revocation.
    let stop = Arc::new(AtomicBool::new(false));
    let mut readers = Vec::new();
    for _ in 0..3 {
        let (w, stop) = (w.clone(), stop.clone());
        readers.push(tokio::spawn(async move {
            // (instant, served) samples, in order.
            let mut seen = Vec::new();
            while !stop.load(Ordering::SeqCst) {
                let served = w.b_serves().await;
                seen.push((Instant::now(), served));
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            seen
        }));
    }
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(w.a_revokes("d1").await, 1);
    let confirmed = Instant::now();
    // Duplicate / replayed revocation: nothing live to revoke.
    assert_eq!(w.a_revokes("d1").await, 0, "the replay confirms nothing");

    let after = w.a.ledger.revocation_watermark().await.unwrap().unwrap();
    assert_eq!(after, before + 1, "one confirmed revocation, one step");

    tokio::time::sleep(Duration::from_millis(1_200)).await;
    stop.store(true, Ordering::SeqCst);

    for reader in readers {
        let seen = reader.await.expect("the reader finishes");
        // Past the bound every sample is a denial, and once a reader saw
        // a denial it never saw the trust again.
        let mut denied = false;
        for (at, served) in &seen {
            if !served {
                denied = true;
            }
            assert!(
                !(denied && *served),
                "a reader saw the revoked trust again after a denial"
            );
            if at.duration_since(confirmed) > Duration::from_millis(100) + SLACK {
                assert!(!served, "served past the bound");
            }
        }
        assert!(denied, "every reader ended up denying");
    }
}

/// Normal path: a re-grant under a NEW decision after a revocation is served
/// by B at once (negatives are never cached), and its own revocation
/// propagates in turn.
#[tokio::test]
async fn a_regrant_under_a_new_decision_is_served_and_revocable_again() {
    let Some(w) = world("regrant", Duration::ZERO).await else {
        return;
    };
    w.a_grants("d1").await;
    assert!(w.b_serves().await);
    assert_eq!(w.a_revokes("d1").await, 1);
    assert!(!w.b_serves().await);

    w.a_grants("d2").await;
    assert!(w.b_serves().await, "the new decision is honored by B");
    assert_eq!(w.a_revokes("d2").await, 1);
    assert!(!w.b_serves().await, "and its revocation propagates too");
}

/// Fail closed: B cut off from the authority never renews a cached ALLOW
/// past the lease; after healing it reads the moved world and stays denied.
#[tokio::test]
async fn a_node_cut_off_from_the_authority_fails_closed_and_does_not_resurrect() {
    let lease = Duration::from_millis(300);
    let Some(w) = world("partition", lease).await else {
        return;
    };
    w.a_grants("d1").await;
    assert!(w.b_serves().await, "B honors it while connected");

    // The partition hits B only; A revokes meanwhile.
    w.b.ledger.partitioned.store(true, Ordering::SeqCst);
    assert_eq!(w.a_revokes("d1").await, 1);

    // Past the lease B cannot verify: it must not serve the cached ALLOW.
    tokio::time::sleep(lease + Duration::from_millis(100)).await;
    let outcome =
        w.b.cache
            .active_grant(&w.account, &w.repo, &w.digest, Utc::now())
            .await;
    assert!(
        !matches!(outcome, Ok(Some(_))),
        "a cut-off node served a trust it cannot verify: {outcome:?}"
    );
    assert!(outcome.is_err(), "no authority, no answer (unavailable)");

    // Healed: B reads the moved world and the revoked trust stays revoked.
    w.b.ledger.partitioned.store(false, Ordering::SeqCst);
    assert!(!w.b_serves().await, "healing does not resurrect the trust");
}

/// The probe fails while the book keeps answering: B must not renew the
/// cached ALLOW; it drops the cache and answers from the live book, which
/// already holds A's revocation.
#[tokio::test]
async fn a_lost_probe_with_a_live_book_answers_from_the_book_not_the_cache() {
    let lease = Duration::from_millis(300);
    let Some(w) = world("probecut", lease).await else {
        return;
    };
    w.a_grants("d1").await;
    assert!(w.b_serves().await);

    w.b.ledger.probe_cut.store(true, Ordering::SeqCst);
    assert_eq!(w.a_revokes("d1").await, 1);
    tokio::time::sleep(lease + Duration::from_millis(100)).await;

    let reads = w.b.reads();
    assert!(
        !w.b_serves().await,
        "a lost probe must not keep serving the cached trust"
    );
    assert!(
        w.b.reads() > reads,
        "the answer came from the live book, not from the cache"
    );
    // Still cut: it keeps reading the live book each time.
    assert!(!w.b_serves().await);
}
