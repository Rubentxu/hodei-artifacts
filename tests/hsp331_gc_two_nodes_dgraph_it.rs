//! HSP-331: two real nodes, one Dgraph, one fenced garbage collection.
//!
//! Two applications are booted with the production `bootstrap` and the real
//! router. They are independent: each one has its own graph connection, its
//! own catalog store, its own `GcService` (its own claim owner and its own
//! in-process gate) and its own HTTP surface. They share only what a real
//! deployment shares: the Dgraph namespace (the authority) and the content
//! directory (the file store).
//!
//! The claim of the platform sweep lives in the authority, so what is proven
//! here is decided by the lease row of the shared Dgraph and not by memory:
//!
//! - **a live lease excludes the other node**: while node A holds the claim
//!   (stalled in the middle of its pass), the sweep of node B answers `409`;
//! - **an expired lease is taken over with a higher generation**: once the
//!   short lease of A runs out, B claims the scope, resumes the durable mark
//!   of A, and its generation is strictly above A's;
//! - **the zombie deletes nothing**: when A wakes up with its old generation
//!   it is fenced (`409`), the blob is still there while B holds the claim, and
//!   only B's generation deletes it, exactly once;
//! - **a partition fails closed**: a node whose route to the Dgraph is cut
//!   cannot claim nor continue a pass (5xx, never a `200`), and the bytes stay.
//!
//! The nodes are stalled with a gate in their content store (before the fenced
//! deletion trace), so the interleaving is decided by the test, not by timing;
//! the only wait on the clock is the lease of A running out (a few seconds,
//! configured with `GcService::with_sweep_lease`).
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing
//! harness fails instead of skipping); serialize with `--test-threads=1`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::gc::GcService;
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{AllowAllAuthorizer, ContentLedger, DeletionState};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig};
use hodei_hub::blob::{BlobError, BlobStore, BlobWriteSession, FileBlobStore, ReadPolicy};
use hodei_hub::{BlobReclaim, ConditionalBlob, ConditionalOutcome, VersionMarker};
use serde_json::Value;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tower::ServiceExt;

#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

/// The scope under which a fenced platform sweep claims its pass.
const PLATFORM_SCOPE: &str = "platform";
const ACCOUNT: &str = "acme";
const TOKEN: &str = "alice@acme";
/// The claim of a node lives this long between renewals. Long enough for the
/// test to act while A still holds it, short enough to wait it out.
const LEASE: Duration = Duration::from_secs(4);

/// Without the harness the suite skips, unless the run demands it.
fn harness_present() -> bool {
    if graph_profile::endpoint().is_some() {
        return true;
    }
    assert!(
        std::env::var("HODEI_REQUIRE_IT").as_deref() != Ok("1"),
        "HODEI_REQUIRE_IT=1 but HODEI_GRAPH_IT_ENDPOINT is not set: a skipped IT is a false green"
    );
    eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
    false
}

// ---------------------------------------------------------------- identity

struct TokenAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for TokenAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let Credentials::Bearer(token) = credentials else {
            return Err(AuthenticationError::InvalidCredentials);
        };
        let (user, account) = token
            .split_once('@')
            .ok_or(AuthenticationError::InvalidCredentials)?;
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{user}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

fn cedar() -> Arc<dyn AuthorizerPort> {
    let policy = format!(
        r#"permit(principal == Iam::User::"hrn:hodei:iam::{ACCOUNT}:User/alice",
       action == Admin::Action::"AccessAdminApi", resource);"#
    );
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            &policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("the policy parses"),
    )))
}

// -------------------------------------------------------- the stalling store

/// A stall point: the node that reaches it announces it and waits to be let
/// go. Armed once; later passes run free.
#[derive(Default)]
struct Gate {
    armed: AtomicBool,
    reached: Notify,
    release: Notify,
}

impl Gate {
    fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    async fn wait_reached(&self) {
        tokio::time::timeout(Duration::from_secs(60), self.reached.notified())
            .await
            .expect("the node reached its stall point");
    }

    fn let_go(&self) {
        self.release.notify_one();
    }
}

/// The real file store, stalling a pass right before it records the fenced
/// deletion trace: after the claim, the mark and the renewal, before a byte is
/// touched. Everything else is delegated untouched.
struct GatedStore {
    inner: FileBlobStore,
    gate: Arc<Gate>,
}

#[async_trait]
impl BlobStore for GatedStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        self.inner.put(bytes).await
    }
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.inner.get(digest).await
    }
    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        self.inner.exists(digest).await
    }
    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        self.inner.begin().await
    }
    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        self.inner.get_with(digest, policy).await
    }
    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        self.inner.length(digest).await
    }
    async fn age(&self, digest: &ContentDigest) -> Result<Option<Duration>, BlobError> {
        self.inner.age(digest).await
    }
    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        self.inner.read_range(digest, offset, length).await
    }
    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        self.inner.verify(digest).await
    }
    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        self.inner.reclaimer()
    }
    fn conditional(&self) -> Option<&dyn ConditionalBlob> {
        Some(self)
    }
}

#[async_trait]
impl ConditionalBlob for GatedStore {
    fn supports_conditional_delete(&self) -> bool {
        true
    }
    async fn head_marker(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<VersionMarker>, BlobError> {
        if self.gate.armed.swap(false, Ordering::SeqCst) {
            self.gate.reached.notify_one();
            self.gate.release.notified().await;
        }
        self.inner.head_marker(digest).await
    }
    async fn delete_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
    ) -> Result<ConditionalOutcome, BlobError> {
        self.inner.delete_if_untouched(digest, marker).await
    }
    async fn quarantine_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
        quarantine_key: &str,
    ) -> Result<ConditionalOutcome, BlobError> {
        self.inner
            .quarantine_if_untouched(digest, marker, quarantine_key)
            .await
    }
}

// ------------------------------------------------------------ the partition

/// A TCP relay between a node and the Dgraph that can be cut: the node's
/// connection dies and nothing listens on the address any more.
struct Relay {
    address: SocketAddr,
    accept: JoinHandle<()>,
    connections: Arc<std::sync::Mutex<Vec<JoinHandle<()>>>>,
}

impl Relay {
    async fn to(upstream: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("relay binds");
        let address = listener.local_addr().expect("relay address");
        let upstream = upstream.to_string();
        let connections: Arc<std::sync::Mutex<Vec<JoinHandle<()>>>> = Arc::default();
        let held = connections.clone();
        let accept = tokio::spawn(async move {
            while let Ok((mut downstream, _)) = listener.accept().await {
                let upstream = upstream.clone();
                let task = tokio::spawn(async move {
                    if let Ok(mut towards) = TcpStream::connect(&upstream).await {
                        let _ = tokio::io::copy_bidirectional(&mut downstream, &mut towards).await;
                    }
                });
                held.lock().expect("relay connections").push(task);
            }
        });
        Self {
            address,
            accept,
            connections,
        }
    }

    /// Cut the route: stop accepting and drop every live connection.
    async fn cut(&self) {
        self.accept.abort();
        for task in self
            .connections
            .lock()
            .expect("relay connections")
            .drain(..)
        {
            task.abort();
        }
        // Let the aborts land before the caller acts.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

// ----------------------------------------------------------------- the nodes

/// One node: its own application over the shared Dgraph namespace.
struct Node {
    app: Router,
    gate: Arc<Gate>,
}

async fn open_store(client: Arc<GraphClient>) -> Arc<DgraphCatalogStore> {
    // `alter` is rejected during the reindexing window of a fresh namespace
    // (transient by contract): retry with a bounded backoff.
    let mut last = String::new();
    for attempt in 0..8u64 {
        match DgraphCatalogStore::open(client.clone(), Arc::new(AllowAllAuthorizer)).await {
            Ok(store) => return Arc::new(store),
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
            }
        }
    }
    panic!("the catalog store opens: {last}");
}

/// Boot a node on `work`'s namespace and content directory. With `via`, its
/// whole route to the Dgraph goes through that address (so it can be cut).
async fn node(work: &Workdir, via: Option<SocketAddr>) -> Node {
    let mut config = AppConfig::default();
    work.apply(&mut config);
    config.security.admin_api_enabled = true;
    if let Some(address) = via {
        let toml = work
            .tenant()
            .graph_toml()
            .replace(&work.tenant().endpoint, &address.to_string());
        config.graph =
            Some(GraphClientConfig::from_toml_str(&toml).expect("the relay graph section"));
    }
    let graph = config.graph.clone().expect("the graph profile");
    let mut client = GraphClient::connect(graph).await.expect("Dgraph connects");
    client.login().await.expect("Dgraph ACL login succeeds");
    let store = open_store(Arc::new(client)).await;

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("the bootstrap");

    let gate = Arc::new(Gate::default());
    let content: Arc<dyn BlobStore> = Arc::new(GatedStore {
        inner: FileBlobStore::new(work.path().join("cas")).expect("the CAS"),
        gate: gate.clone(),
    });
    let ledger: Arc<dyn ContentLedger> = store;
    let gc = Arc::new(
        GcService::new(ledger, content, Duration::ZERO)
            .expect("the GC service")
            .with_sweep_lease(LEASE),
    );
    let state = state
        .with_gc(gc)
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar());
    Node {
        app: build_router(state, &config),
        gate,
    }
}

/// Start `POST /api/v1/gc/sweep` on a node, detached: the answer comes later.
fn sweep(node: &Node) -> JoinHandle<(StatusCode, Value)> {
    let app = node.app.clone();
    tokio::spawn(async move {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/api/v1/gc/sweep")
            .header(header::HOST, "127.0.0.1")
            .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(r#"{"execute":true}"#))
            .expect("the request");
        let response = app.oneshot(request).await.expect("a response");
        let status = response.status();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    })
}

async fn answer(task: JoinHandle<(StatusCode, Value)>) -> (StatusCode, Value) {
    tokio::time::timeout(Duration::from_secs(90), task)
        .await
        .expect("the sweep answered in time")
        .expect("the sweep task")
}

// ------------------------------------------------------------ the observer

/// The shared authority as an outside observer sees it.
struct Observer {
    ledger: Arc<DgraphCatalogStore>,
    cas: PathBuf,
}

impl Observer {
    async fn new(work: &Workdir) -> Self {
        Self {
            ledger: Arc::new(work.tenant().catalog_store().await),
            cas: work.path().join("cas"),
        }
    }

    /// The live claim of the platform scope, if any.
    async fn lease(&self) -> Option<hodei_catalog::GcLease> {
        ContentLedger::current_gc_lease(&*self.ledger, PLATFORM_SCOPE, Utc::now())
            .await
            .expect("the lease is readable")
    }

    async fn deletion(&self, digest: &ContentDigest) -> Option<hodei_catalog::GcDeletion> {
        ContentLedger::deletion_of(&*self.ledger, digest)
            .await
            .expect("the trace is readable")
    }

    /// Leave bytes in the store that no catalog names: a real orphan.
    fn plant_orphan(&self, label: &str) -> ContentDigest {
        let content = format!(
            "hsp331-orphan-{label}-{}",
            Utc::now().timestamp_nanos_opt().unwrap()
        );
        let digest = ContentDigest::of_bytes(content.as_bytes());
        let path = self.blob_path(&digest);
        std::fs::create_dir_all(path.parent().expect("a blob directory")).expect("blob directory");
        std::fs::write(&path, content).expect("the orphan is written");
        digest
    }

    fn blob_path(&self, digest: &ContentDigest) -> PathBuf {
        let value = &digest.value;
        Path::new(&self.cas)
            .join(&value[..2])
            .join(&value[2..4])
            .join(value)
    }

    fn present(&self, digest: &ContentDigest) -> bool {
        self.blob_path(digest).exists()
    }
}

// --------------------------------------------------------------------- tests

/// A live lease excludes the other node, an expired one is taken over with a
/// higher generation, and the zombie of the old generation deletes nothing.
#[tokio::test]
async fn a_live_lease_excludes_the_other_node_and_the_zombie_deletes_nothing() {
    if !harness_present() {
        return;
    }
    let work = Workdir::new();
    let a = node(&work, None).await;
    let b = node(&work, None).await;
    let seen = Observer::new(&work).await;
    let orphan = seen.plant_orphan("takeover");

    // Node A starts its pass and stalls holding the claim, with the mark
    // durable and no byte touched yet.
    a.gate.arm();
    let zombie = sweep(&a);
    a.gate.wait_reached().await;
    let lease_a = seen.lease().await.expect("A holds a live claim");
    assert!(seen.present(&orphan), "nothing was deleted yet");

    // (1) The claim is live: B is excluded by the authority, with a 409.
    let (status, body) = answer(sweep(&b)).await;
    assert_eq!(status, StatusCode::CONFLICT, "B while A is live: {body}");
    assert!(seen.present(&orphan), "the refused node touched nothing");
    assert_eq!(
        seen.lease().await.expect("the claim is still A's").token,
        lease_a.token,
        "the refused claim did not disturb A's"
    );

    // (2) A stays stalled until its lease runs out; the authority, not A,
    // decides it is over.
    let waited = tokio::time::Instant::now();
    while seen.lease().await.is_some() {
        assert!(
            waited.elapsed() < LEASE + Duration::from_secs(20),
            "the lease of A never expired"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // B takes over: it stalls at the same point, claim held, so the
    // generation it got can be read from the authority.
    b.gate.arm();
    let taker = sweep(&b);
    b.gate.wait_reached().await;
    let lease_b = seen.lease().await.expect("B holds a live claim");
    assert!(
        lease_b.generation > lease_a.generation,
        "the take-over raises the generation: A {} -> B {}",
        lease_a.generation,
        lease_b.generation
    );
    assert_ne!(
        lease_b.token, lease_a.token,
        "the take-over has a new token"
    );

    // (3) The zombie wakes up holding generation {A}: it is fenced and the
    // byte it meant to sweep is still there, because B has not deleted it yet.
    a.gate.let_go();
    let (status, body) = answer(zombie).await;
    assert_eq!(status, StatusCode::CONFLICT, "the zombie is fenced: {body}");
    assert!(seen.present(&orphan), "the zombie deleted nothing");
    assert!(
        seen.deletion(&orphan).await.is_none(),
        "the zombie left no deletion trace"
    );
    assert_eq!(
        seen.lease().await.expect("B still holds").token,
        lease_b.token,
        "the zombie did not take the claim back"
    );

    // Only the new generation deletes, and it resumes the durable mark of A.
    b.gate.let_go();
    let (status, body) = answer(taker).await;
    assert_eq!(status, StatusCode::OK, "B finishes: {body}");
    assert_eq!(body["report"]["deleted"], 1, "exactly one deletion: {body}");
    assert_eq!(
        body["report"]["resumed"], true,
        "B resumed A's durable mark: {body}"
    );
    assert!(!seen.present(&orphan), "the orphan was reclaimed by B");
    let trace = seen.deletion(&orphan).await.expect("the deletion trace");
    assert_eq!(trace.state, DeletionState::Confirmed);
}

/// A node cut off from the Dgraph fails closed: it cannot claim, and a pass
/// that was running cannot continue. The bytes stay and the others sweep.
#[tokio::test]
async fn a_partitioned_node_fails_closed() {
    if !harness_present() {
        return;
    }
    let work = Workdir::new();
    let endpoint = work.tenant().endpoint.clone();
    let relay = Relay::to(&endpoint).await;
    let cut_off = node(&work, Some(relay.address)).await;
    let healthy = node(&work, None).await;
    let seen = Observer::new(&work).await;
    let orphan = seen.plant_orphan("partition");

    // The node has a pass in flight (claim held) when the route is cut.
    cut_off.gate.arm();
    let in_flight = sweep(&cut_off);
    cut_off.gate.wait_reached().await;
    assert!(seen.lease().await.is_some(), "the node holds the claim");
    relay.cut().await;
    cut_off.gate.let_go();
    let (status, body) = answer(in_flight).await;
    assert!(
        status.is_server_error() || status == StatusCode::CONFLICT,
        "a cut-off pass must not succeed: {status} {body}"
    );
    assert_ne!(status, StatusCode::OK);
    assert!(seen.present(&orphan), "the cut-off pass deleted nothing");
    assert!(
        seen.deletion(&orphan).await.is_none(),
        "and recorded nothing"
    );

    // A new request on the cut-off node cannot even claim.
    let (status, body) = answer(sweep(&cut_off)).await;
    assert!(
        status.is_server_error(),
        "without the authority the claim fails closed: {status} {body}"
    );
    assert!(seen.present(&orphan), "the bytes survive the partition");

    // The partition is the node's: the rest of the platform is unharmed and
    // sweeps once the dead node's claim runs out.
    let waited = tokio::time::Instant::now();
    loop {
        let (status, body) = answer(sweep(&healthy)).await;
        if status == StatusCode::OK {
            assert_eq!(body["report"]["deleted"], 1, "{body}");
            break;
        }
        assert_eq!(
            status,
            StatusCode::CONFLICT,
            "only the stale claim blocks: {body}"
        );
        assert!(
            waited.elapsed() < LEASE + Duration::from_secs(20),
            "the healthy node never took over"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(
        !seen.present(&orphan),
        "the healthy node reclaimed the orphan"
    );
}
