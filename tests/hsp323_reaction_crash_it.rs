//! HSP-78 AC1/AC2 (slice of HSP-323): the REAL Artifacts binary is killed
//! (`SIGKILL`) at an exact point of the reactive cycle and restarted over the
//! same Dgraph.
//!
//! Gated on HODEI_GRAPH_IT_ENDPOINT (`host:port`, no scheme, no TLS). The
//! service under test is the production binary on `RUN_MODE=dgraph`; there is
//! NO fault-injection code in it. The exact stop point is made from OUTSIDE:
//! the binary talks to Dgraph through a gRPC-transparent TCP gate owned by the
//! test. The gate reads the HTTP/2 frames of the client, recognizes the write
//! of the step it is armed for by its N-Quads, and either
//!
//! - holds the write back (the process dies BEFORE the step reaches Dgraph), or
//! - lets the write through and withholds every answer (the process dies AFTER
//!   Dgraph committed the step and BEFORE the process learned about it).
//!
//! The test then waits (bounded polling, no fixed sleeps) until the gate
//! tripped, checks the durable state from an independent connection, kills the
//! process with `SIGKILL`, restarts it and measures what AC2 says:
//!
//! - the pending intents are recovered and applied;
//! - the external effect (the security case write, counted at the gate and in
//!   Dgraph) happens ONCE, never twice;
//! - an intent whose receipt is `unknown` is NOT re-executed by the restart,
//!   and stays reconcilable.
//!
//! Points covered: before the intent is said to the outbox ("reserve"), before
//! the claim reaches Dgraph, after the claim and before the effect, and after
//! the external effect and before the receipt.

mod support;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hodei_evidence::ContentDigest;
use hodei_graph_client::{GraphClient, GraphClientConfig, schema::ensure_ready};
use hodei_reactive::intent::{
    EventOrigin, Intent, IntentOrigin, IntentOutbox, IntentStatus, IntentStatusSource, Receipt,
};
use hodei_trust_policy::{Requirement, RequirementKind, TrustPolicy};
use hodei_trust_policy_storage::DgraphIntentStore;
use serde_json::{Value, json};
use support::{Curl, Deployment, Service};

/// The N-Quads of the write that says an intent to the outbox.
const SAY_MARKER: &[u8] = b"_:intent <dgraph.type> \"HodeiReactiveIntent\"";
/// The N-Quads of the write that claims an intent.
const CLAIM_MARKER: &[u8] = b"_:claim <dgraph.type> \"HodeiIntentClaim\"";
/// The N-Quads of the external effect: the write of the security case.
const EFFECT_MARKER: &[u8] = b"_:case <dgraph.type> \"HodeiSecurityCase\"";

/// The lease the runtime gives a claim (seconds), plus a margin.
const LEASE_SECONDS: i64 = 60;

const INVENTORY: &str = r#"{
  "bomFormat": "CycloneDX",
  "specVersion": "1.7",
  "version": 1,
  "metadata": {
    "timestamp": "2026-03-04T10:15:00Z",
    "component": {
      "bom-ref": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06",
      "type": "container",
      "name": "web",
      "version": "1.0.0",
      "purl": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06"
    }
  },
  "components": [
    { "bom-ref": "pkg:maven/org.example/lib-a@1.2.3", "type": "library", "name": "lib-a", "version": "1.2.3", "purl": "pkg:maven/org.example/lib-a@1.2.3" }
  ],
  "dependencies": [
    { "ref": "pkg:oci/web@sha256:ac9da485d6eac8c161117ecd4155cf50bdb330c8241234d669aef1edba265a06", "dependsOn": ["pkg:maven/org.example/lib-a@1.2.3"] }
  ]
}"#;

// ---------------------------------------------------------------------------
// The gate: a gRPC-transparent TCP proxy that stops the process at a step.
// ---------------------------------------------------------------------------

/// The step of the cycle the gate stops at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Point {
    /// The intent is about to be said to the outbox: held back.
    BeforeSay,
    /// The claim is about to reach Dgraph: held back.
    BeforeClaim,
    /// The claim committed; the process never hears it.
    AfterClaim,
    /// The external effect committed; the process never hears it, and the
    /// receipt is never written.
    AfterEffect,
}

impl Point {
    fn marker(self) -> &'static [u8] {
        match self {
            Point::BeforeSay => SAY_MARKER,
            Point::BeforeClaim | Point::AfterClaim => CLAIM_MARKER,
            Point::AfterEffect => EFFECT_MARKER,
        }
    }

    /// Whether the write goes through before the process is stopped.
    fn lets_it_through(self) -> bool {
        matches!(self, Point::AfterClaim | Point::AfterEffect)
    }
}

struct Gate {
    target: String,
    armed: Mutex<Option<Point>>,
    /// Every answer is withheld from here on.
    mute: AtomicBool,
    /// Nothing is forwarded from here on.
    frozen: AtomicBool,
    tripped: AtomicBool,
    closing: AtomicBool,
    /// The writes of the external effect that were forwarded to Dgraph.
    effect_writes: AtomicUsize,
}

struct GateHandle {
    gate: Arc<Gate>,
    port: u16,
}

impl GateHandle {
    fn open(target: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("the gate binds");
        let port = listener.local_addr().expect("the gate address").port();
        let gate = Arc::new(Gate {
            target: target.to_string(),
            armed: Mutex::new(None),
            mute: AtomicBool::new(false),
            frozen: AtomicBool::new(false),
            tripped: AtomicBool::new(false),
            closing: AtomicBool::new(false),
            effect_writes: AtomicUsize::new(0),
        });
        let shared = gate.clone();
        std::thread::spawn(move || {
            for incoming in listener.incoming() {
                if shared.closing.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(client) = incoming else { continue };
                let Ok(upstream) = TcpStream::connect(&shared.target) else {
                    continue;
                };
                let (Ok(client_back), Ok(upstream_back)) =
                    (client.try_clone(), upstream.try_clone())
                else {
                    continue;
                };
                let forward = shared.clone();
                std::thread::spawn(move || requests(forward, client, upstream));
                let backward = shared.clone();
                std::thread::spawn(move || answers(backward, upstream_back, client_back));
            }
        });
        Self { gate, port }
    }

    fn address(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    fn arm(&self, point: Point) {
        *self.gate.armed.lock().expect("the gate lock") = Some(point);
    }

    fn disarm(&self) {
        *self.gate.armed.lock().expect("the gate lock") = None;
        self.gate.mute.store(false, Ordering::SeqCst);
        self.gate.frozen.store(false, Ordering::SeqCst);
        self.gate.tripped.store(false, Ordering::SeqCst);
    }

    fn tripped(&self) -> bool {
        self.gate.tripped.load(Ordering::SeqCst)
    }

    fn effect_writes(&self) -> usize {
        self.gate.effect_writes.load(Ordering::SeqCst)
    }
}

impl Drop for GateHandle {
    fn drop(&mut self) {
        self.gate.closing.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
    }
}

/// Dgraph to client: copies bytes until the gate mutes the answers.
fn answers(gate: Arc<Gate>, mut from: TcpStream, mut to: TcpStream) {
    let mut buffer = [0u8; 16 * 1024];
    loop {
        match from.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(read) => {
                if gate.closing.load(Ordering::SeqCst) {
                    return;
                }
                if gate.mute.load(Ordering::SeqCst) {
                    continue;
                }
                if to.write_all(&buffer[..read]).is_err() {
                    return;
                }
            }
        }
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Client to Dgraph: HTTP/2 frames, with the DATA payloads inspected.
fn requests(gate: Arc<Gate>, mut client: TcpStream, mut upstream: TcpStream) {
    // The connection preface.
    let mut preface = [0u8; 24];
    if client.read_exact(&mut preface).is_err() || upstream.write_all(&preface).is_err() {
        return;
    }
    // The stream whose end must still be forwarded after the trigger.
    let mut finishing: Option<u32> = None;
    loop {
        let mut header = [0u8; 9];
        if client.read_exact(&mut header).is_err() {
            return;
        }
        let length = u32::from_be_bytes([0, header[0], header[1], header[2]]) as usize;
        let kind = header[3];
        let flags = header[4];
        let stream = u32::from_be_bytes([header[5] & 0x7f, header[6], header[7], header[8]]);
        let mut payload = vec![0u8; length];
        if client.read_exact(&mut payload).is_err() {
            return;
        }
        if gate.closing.load(Ordering::SeqCst) {
            return;
        }
        if gate.frozen.load(Ordering::SeqCst) {
            // The process is dead in everything but name: nothing moves.
            continue;
        }
        let is_data = kind == 0;
        let mut trips = false;
        if is_data {
            if contains(&payload, EFFECT_MARKER) {
                gate.effect_writes.fetch_add(1, Ordering::SeqCst);
            }
            let armed = *gate.armed.lock().expect("the gate lock");
            if let Some(point) = armed
                && !gate.tripped.load(Ordering::SeqCst)
                && contains(&payload, point.marker())
            {
                if !point.lets_it_through() {
                    // Held back: the write never reaches Dgraph.
                    if contains(&payload, EFFECT_MARKER) {
                        gate.effect_writes.fetch_sub(1, Ordering::SeqCst);
                    }
                    gate.mute.store(true, Ordering::SeqCst);
                    gate.frozen.store(true, Ordering::SeqCst);
                    gate.tripped.store(true, Ordering::SeqCst);
                    continue;
                }
                // Let it through, but the process hears nothing back.
                gate.mute.store(true, Ordering::SeqCst);
                trips = true;
            }
        }
        let mut frame = header.to_vec();
        frame.extend_from_slice(&payload);
        if upstream.write_all(&frame).is_err() {
            return;
        }
        if trips {
            if flags & 0x1 != 0 {
                gate.frozen.store(true, Ordering::SeqCst);
                gate.tripped.store(true, Ordering::SeqCst);
            } else {
                finishing = Some(stream);
            }
        } else if is_data && flags & 0x1 != 0 && finishing == Some(stream) {
            gate.frozen.store(true, Ordering::SeqCst);
            gate.tripped.store(true, Ordering::SeqCst);
        }
    }
}

// ---------------------------------------------------------------------------
// The world.
// ---------------------------------------------------------------------------

fn nonce() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos()
        .to_string()
}

fn graph_config() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let mut text =
        format!("endpoint = \"{endpoint}\"\n\n[tls]\nenabled = false\nca_cert_path = \"\"\n");
    if let (Ok(user), Ok(password)) = (
        std::env::var("HODEI_GRAPH_IT_USER"),
        std::env::var("HODEI_GRAPH_IT_PASSWORD"),
    ) {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(GraphClientConfig::from_toml_str(&text).expect("the IT configuration validates"))
}

/// An independent connection straight to the alpha, to measure from outside.
async fn fresh_client() -> Arc<GraphClient> {
    let mut client = GraphClient::connect(graph_config().expect("gated"))
        .await
        .expect("Dgraph connects");
    if std::env::var("HODEI_GRAPH_IT_USER").is_ok() {
        client.login().await.expect("Dgraph ACL login succeeds");
    }
    ensure_ready(&client).await.expect("the schema boots");
    Arc::new(client)
}

fn osv(id: &str) -> Value {
    json!({
        "schema_version": "1.6.0",
        "id": id,
        "modified": "2026-11-18T00:00:00Z",
        "affected": [
            { "package": { "ecosystem": "Maven", "name": "lib-a", "purl": "pkg:maven/org.example/lib-a@1.2.3" } }
        ]
    })
}

fn the_schema() -> String {
    let mut schema: Value = serde_json::from_str(support::SCHEMA).expect("the schema parses");
    schema["Admin"] = json!({
        "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
        "actions": {
            "AccessAdminApi": {
                "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] }
            }
        }
    });
    schema.to_string()
}

fn the_policy() -> String {
    // The measurement is of the crash recovery, not of Cedar: alice may do anything.
    "permit(principal, action, resource);\n".to_string()
}

fn call(
    deployment: &Deployment,
    port: u16,
    method: &str,
    path: &str,
    token: &str,
    body: Option<&Value>,
) -> (u16, String) {
    let directory = deployment.directory.path();
    let into = directory.join("response.bin");
    let _ = std::fs::remove_file(&into);
    let sent = directory.join("request.bin");
    let (file, content_type) = match body {
        Some(value) => {
            std::fs::write(&sent, value.to_string()).expect("the body is written");
            (Some(sent.as_path()), Some("application/json"))
        }
        None => (None, None),
    };
    let answer = Curl::call(port, method, path, token, file, content_type, Some(&into)).run();
    let text = String::from_utf8_lossy(&std::fs::read(&into).unwrap_or_default()).into_owned();
    (answer.status, text)
}

/// Waits, polling a bounded number of times, until `check` holds.
async fn eventually<F, Fut>(what: &str, seconds: u64, mut check: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        if check().await {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out after {seconds}s waiting for: {what}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

struct World {
    deployment: Deployment,
    gate: GateHandle,
    environment: Vec<(String, String)>,
    direct: Arc<GraphClient>,
    store: DgraphIntentStore,
    account: String,
    repository: String,
    digest: ContentDigest,
    advisory_id: String,
    token: String,
}

impl World {
    async fn new() -> Self {
        let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").expect("gated");
        let tag = nonce();
        let account = format!("c323a{}", &tag[8..]);
        let advisory_id = format!("GHSA-c323-{}", &tag[8..]);
        let digest = ContentDigest::of_bytes(format!("hsp323-crash-{advisory_id}").as_bytes());
        let deployment = Deployment::new();
        std::fs::write(
            deployment.directory.path().join("schema.json"),
            the_schema(),
        )
        .expect("schema");
        std::fs::write(
            deployment.directory.path().join("policies.cedar"),
            the_policy(),
        )
        .expect("policy");
        let policy = TrustPolicy::new(
            "default",
            "1.0.0",
            vec![Requirement::denying(
                RequirementKind::NoReachableCriticalVulnerability,
            )],
        );
        let policy_path: PathBuf = deployment.directory.path().join("trust-policy.json");
        std::fs::write(&policy_path, serde_json::to_vec(&policy).expect("json"))
            .expect("policy file");
        let catalog_path = deployment.directory.path().join("risk-catalog.json");
        std::fs::write(&catalog_path, "{}").expect("catalog file");

        let gate = GateHandle::open(&endpoint);
        let mut environment: Vec<(String, String)> = vec![
            ("RUN_MODE".into(), "dgraph".into()),
            ("HODEI_LOGGING__FORMAT".into(), "pretty".into()),
            ("HODEI_GRAPH__ENDPOINT".into(), gate.address()),
            ("HODEI_GRAPH__TLS__ENABLED".into(), "false".into()),
            ("HODEI_GRAPH__TLS__CA_CERT_PATH".into(), String::new()),
            ("HODEI_GRAPH__TLS__SERVER_NAME".into(), String::new()),
            ("HODEI_GRAPH__NAMESPACE".into(), "0".into()),
            ("HODEI_CONTENT__BACKEND".into(), "filesystem".into()),
            ("HODEI_SECURITY__ADMIN_API_ENABLED".into(), "true".into()),
            (
                "HODEI_TRUST__POLICY_FILE".into(),
                policy_path.to_string_lossy().into_owned(),
            ),
            (
                "HODEI_TRUST__RISK_CATALOG_FILE".into(),
                catalog_path.to_string_lossy().into_owned(),
            ),
        ];
        if let (Ok(user), Ok(password)) = (
            std::env::var("HODEI_GRAPH_IT_USER"),
            std::env::var("HODEI_GRAPH_IT_PASSWORD"),
        ) {
            environment.push(("HODEI_GRAPH__CREDENTIALS__USERID".into(), user));
            environment.push(("HODEI_GRAPH__CREDENTIALS__PASSWORD".into(), password));
        }
        let direct = fresh_client().await;
        let store = DgraphIntentStore::open(direct.clone())
            .await
            .expect("the intent store opens");
        let token = support::token(&account, "alice");
        Self {
            deployment,
            gate,
            environment,
            direct,
            store,
            account,
            repository: "releases".to_string(),
            digest,
            advisory_id,
            token,
        }
    }

    fn boot(&self) -> Service {
        let refs: Vec<(&str, &str)> = self
            .environment
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        self.deployment.start(&refs)
    }

    fn log(&self) -> String {
        support::without_the_colors(&self.deployment.the_log())
    }

    /// Creates the repository and publishes the subject's bytes.
    fn deploy(&self, port: u16) {
        let (created, body) = call(
            &self.deployment,
            port,
            "POST",
            "/api/v1/catalog/repositories",
            &self.token,
            Some(&json!({"name": self.repository, "format": "raw", "mode": "hosted"})),
        );
        assert_eq!(created, 201, "the repository: {body}\n{}", self.log());
        let file = self.deployment.directory.path().join("app.bin");
        let content = format!("hsp323-crash-{}", self.advisory_id).into_bytes();
        std::fs::write(&file, &content).expect("the blob");
        // The digest of the subject is the one of the published bytes.
        let published = support::curl_upload(
            port,
            &format!("/repository/{}/dir/app.bin", self.repository),
            &self.token,
            &file,
            None,
        );
        assert!(
            published.status == 200 || published.status == 201,
            "the publication: {}\n{}",
            published.status,
            self.log()
        );
    }

    fn accept_request(&self) -> Value {
        json!({
            "repository": self.repository,
            "digest": format!("sha256:{}", self.digest.value),
            "advisory": osv(&self.advisory_id),
            "inventory": serde_json::from_str::<Value>(INVENTORY).expect("inventory"),
        })
    }

    /// Posts the advisory and waits for the answer.
    fn accept(&self, port: u16) -> (u16, String) {
        call(
            &self.deployment,
            port,
            "POST",
            "/api/v1/advisories/accept",
            &self.token,
            Some(&self.accept_request()),
        )
    }

    /// Posts the advisory without waiting: the gate will hold the cycle.
    fn accept_in_background(&self, port: u16) -> std::process::Child {
        let directory = self.deployment.directory.path();
        let sent = directory.join("accept-background.bin");
        std::fs::write(&sent, self.accept_request().to_string()).expect("the body is written");
        Curl::call(
            port,
            "POST",
            "/api/v1/advisories/accept",
            &self.token,
            Some(sent.as_path()),
            Some("application/json"),
            Some(&directory.join("accept-background-response.bin")),
        )
        .run_in_background()
    }

    /// The intents this run's advisory said, as the outbox reads them.
    async fn intents(&self) -> Vec<IntentStatus> {
        let scan = self
            .store
            .scan_intents(5000)
            .await
            .expect("the outbox is readable");
        assert!(!scan.truncated, "the scan must not be truncated");
        scan.rows
            .into_iter()
            .filter(|row| row.intent.identity.contains(&self.advisory_id))
            .collect()
    }

    /// The security cases of this run's advisory that exist in Dgraph.
    async fn cases(&self) -> usize {
        let response = self
            .direct
            .execute_dql("{ q(func: type(HodeiSecurityCase)) { hodei.case.key } }")
            .await
            .expect("the cases are readable");
        response.json["q"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .filter(|row| {
                        row["hodei.case.key"]
                            .as_str()
                            .is_some_and(|key| key.contains(&self.advisory_id))
                    })
                    .count()
            })
            .unwrap_or(0)
    }
}

fn states(rows: &[IntentStatus]) -> Vec<(String, String, String)> {
    rows.iter()
        .map(|row| {
            (
                row.intent.kind.clone(),
                row.state.clone(),
                row.intent.identity.clone(),
            )
        })
        .collect()
}

/// Kills the process with `SIGKILL` and reaps the background request.
fn kill_and_reap(service: &mut Service, request: std::process::Child) {
    support::kill(service);
    let _ = request.wait_with_output();
}

/// The whole scenario of one point.
async fn crash_at(point: Point) {
    if graph_config().is_none() {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    }
    let world = World::new().await;
    let mut service = world.boot();
    let port = service.port;
    world.deploy(port);

    // The gate is armed for the step, and the cycle starts.
    world.gate.arm(point);
    let request = world.accept_in_background(port);
    eventually("the gate to trip at the armed step", 90, || async {
        world.gate.tripped()
    })
    .await;
    let tripped_at = chrono::Utc::now();

    // The durable state at the moment of death, read from outside.
    match point {
        Point::BeforeSay => {
            assert!(
                world.intents().await.is_empty(),
                "nothing was said before the stop: {:?}",
                states(&world.intents().await)
            );
        }
        Point::BeforeClaim => {
            let rows = world.intents().await;
            assert!(!rows.is_empty(), "the plan was said before the stop");
            assert!(
                rows.iter().all(|row| row.state == "pending"),
                "nothing was claimed before the stop: {:?}",
                states(&rows)
            );
        }
        Point::AfterClaim => {
            eventually("the claim to be durable", 30, || async {
                world
                    .intents()
                    .await
                    .iter()
                    .any(|row| row.state == "claimed")
            })
            .await;
            assert_eq!(world.cases().await, 0, "the effect had not happened yet");
        }
        Point::AfterEffect => {
            eventually("the external effect to be durable", 30, || async {
                world.cases().await == 1
            })
            .await;
            let rows = world.intents().await;
            let case = rows
                .iter()
                .find(|row| row.intent.kind == "security_case")
                .expect("the case intent exists");
            assert_eq!(
                case.state,
                "claimed",
                "the effect landed but its receipt did not: {:?}",
                states(&rows)
            );
            assert_eq!(world.gate.effect_writes(), 1);
        }
    }

    // SIGKILL: no chance to clean anything up.
    kill_and_reap(&mut service, request);
    world.gate.disarm();

    // A claimed intent is somebody else's until its lease dies: the restart
    // recovers it only after the lease. (The runtime fixes it at 60 s.)
    if matches!(point, Point::AfterClaim | Point::AfterEffect) {
        let free_at = tripped_at + chrono::Duration::seconds(LEASE_SECONDS + 2);
        eventually("the lease of the dead worker to expire", 120, || async {
            chrono::Utc::now() >= free_at
        })
        .await;
    }

    // The restart: the same binary over the same Dgraph.
    let restarted = world.boot();
    let port = restarted.port;
    if point == Point::BeforeSay {
        // Nothing was said, so there is nothing to resume: the caller that
        // never got an answer repeats the request.
        let (status, body) = world.accept(port);
        assert!(
            (200..300).contains(&status),
            "the repeated request: {status} {body}\n{}",
            world.log()
        );
        let said = world.intents().await;
        assert!(
            !said.is_empty(),
            "the repeated request said no intent: the plan was recorded before the \
             intents were said, so the repetition is a duplicate and the plan is lost \
             (answer: {body})"
        );
    }
    eventually("every intent to leave pending and claimed", 90, || async {
        let rows = world.intents().await;
        !rows.is_empty()
            && rows
                .iter()
                .all(|row| row.state != "pending" && row.state != "claimed")
    })
    .await;

    let rows = world.intents().await;
    let case = rows
        .iter()
        .find(|row| row.intent.kind == "security_case")
        .unwrap_or_else(|| panic!("the case intent was lost: {:?}", states(&rows)));
    assert_eq!(
        case.state,
        "applied",
        "the recovered effect is applied: {:?}\n{}",
        states(&rows),
        world.log()
    );
    // The external effect happened once: at the gate and in Dgraph.
    assert_eq!(
        world.gate.effect_writes(),
        1,
        "the external effect was sent exactly once\n{}",
        world.log()
    );
    assert_eq!(world.cases().await, 1, "there is exactly one case");
    drop(restarted);
}

/// The runtime says the intents to the outbox BEFORE it records the plan as
/// Completed, so a crash between the two leaves no Completed plan: the
/// repeated request plans again and the effect is recovered.
#[tokio::test]
async fn a_crash_before_the_intent_is_said_loses_nothing() {
    crash_at(Point::BeforeSay).await;
}

#[tokio::test]
async fn a_crash_before_the_claim_is_recovered_and_applied_once() {
    crash_at(Point::BeforeClaim).await;
}

#[tokio::test]
async fn a_crash_after_the_claim_is_recovered_and_applied_once() {
    crash_at(Point::AfterClaim).await;
}

#[tokio::test]
async fn a_crash_after_the_effect_and_before_the_receipt_does_not_repeat_it() {
    crash_at(Point::AfterEffect).await;
}

/// What the domain calls uncertain survives a restart marked, is not
/// executed again, and is reconcilable.
#[tokio::test]
async fn an_unknown_effect_survives_a_restart_and_stays_reconcilable() {
    if graph_config().is_none() {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set");
        return;
    }
    let world = World::new().await;
    let digest = ContentDigest::of_bytes(world.advisory_id.as_bytes());
    let subject = hodei_artifacts_api::trust::hrn::artifact_hrn(
        "eu",
        &world.account,
        &world.repository,
        &digest,
    );
    let uncertain = Intent::new(
        IntentOrigin {
            behavior: "planned_vulnerability_reaction".to_string(),
            event: EventOrigin::new(
                format!("evt-{}", world.advisory_id),
                vec![format!("evt-{}", world.advisory_id)],
            ),
        },
        "hodei",
        "decision_reevaluation",
        format!("decision:{}:uncertain", world.advisory_id),
        "v1",
        json!({ "subject": subject.to_string(), "reason": "vulnerability_impacted" }),
    );
    world
        .store
        .put_pending(&uncertain)
        .await
        .expect("the effect is said");
    let now = chrono::Utc::now();
    world
        .store
        .claim(
            &uncertain.identity,
            "worker-dead",
            now + chrono::Duration::seconds(LEASE_SECONDS),
            now,
        )
        .await
        .expect("the claim answers")
        .expect("the claim is won");
    world
        .store
        .record_receipt(
            &uncertain.identity,
            &Receipt::Unknown {
                reason: "sent, and the answer never came".to_string(),
            },
        )
        .await
        .expect("the receipt is written");

    // The binary boots, resumes at startup, and must not touch it.
    let service = world.boot();
    let again = world.intents().await;
    assert_eq!(again.len(), 1, "{:?}", states(&again));
    assert_eq!(again[0].state, "unknown", "marked, not re-executed");
    assert_eq!(
        again[0].receipt,
        Some(Receipt::Unknown {
            reason: "sent, and the answer never came".to_string()
        })
    );
    drop(service);

    // Reconciliation is explicit, and the only way back to pending.
    world
        .store
        .reconcile(&uncertain.identity)
        .await
        .expect("the reconciliation is accepted");
    let rows = world.intents().await;
    assert_eq!(rows[0].state, "pending", "{:?}", states(&rows));
}
