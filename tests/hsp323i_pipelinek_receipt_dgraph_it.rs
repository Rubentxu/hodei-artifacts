//! HSP-323 slice 8 (origin HSP-37): the adapter that exports the execution
//! receipt of a REAL PipelineK run, and the path that links it to the artifact
//! by exact identity, through the real router over the real Dgraph evidence
//! store, the real Cedar engine, the real GC service and the real file CAS.
//!
//! The pipelines are run by the installed `pipelinek` binary (the public CLI:
//! `run`, `events --typed`, `version --json`), pinned to the version the
//! adapter accepts. No `pipelinek serve`, no Fabric: the whole path works with
//! the Fabric stopped because nothing in it talks to one.
//!
//! What each case proves:
//!
//! - AC1/AC2 `a_real_pipelinek_run_is_exported_and_linked_by_exact_identity`:
//!   the receipt exported from a real run carries the run id PipelineK minted,
//!   the declared digest and revision; Hodei links it to
//!   `artifact/<repository>/sha256-<hex>` and the trust capsule of that exact
//!   identity shows the evidence; the control is the capsule of another
//!   artifact, which shows none.
//! - AC4 `the_same_receipt_three_times_is_one_logical_receipt`: three
//!   deliveries, one record (`created`, then `already_accepted` twice, same
//!   evidence id, one link in the capsule, one row in the store).
//! - AC5 `two_runs_with_the_same_digest_are_both_linked_independently`: two
//!   real runs that built the same bytes give two records with their own run
//!   ids; both are linked and neither replaces the other.
//! - AC3 `a_digest_that_is_not_the_bytes_is_rejected_and_publishes_no_lineage`:
//!   a receipt of a real run whose digest is not the digest of the bytes the
//!   service holds is 422 and nothing is recorded; the control is the same
//!   receipt with the true digest, which links.
//! - AC6 `no_secret_reaches_a_receipt_the_answers_or_the_audit`: the real
//!   pipeline prints a secret; it is in no receipt, no answer, no audit event
//!   and not in the stored evidence content, and a receipt that carries a
//!   secret in a field is refused without echoing it.
//! - Fail closed and isolation `refusals_are_closed_and_accounts_are_isolated`:
//!   no Cedar action is a 403 (audited) and writes nothing; a failed run, a
//!   foreign producer version and an unknown field are refused; another
//!   account's repository is a 404 and a stranger who ingests the same run id
//!   and digest in their own repository never touches the owner's evidence.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing
//! harness or a missing `pipelinek` fails instead of skipping); serialize with
//! `--test-threads=1`.

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use cedar_policy::Entities;
use chrono::Utc;
use hodei_artifacts_api::audit::InMemoryAuditSink;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::execution_receipt::{
    ExecutionReceiptV1, PINNED_PIPELINEK_VERSION, PipelinekExport, ReceiptLimits,
    export_from_pipelinek,
};
use hodei_artifacts_api::gc::GcService;
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::{
    CatalogReader, CatalogWriter, ContentLedger, DigestScope, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_evidence::ports::EvidenceStore;
use hodei_evidence_storage::DgraphEvidenceStore;
use hodei_hub::blob::{BlobStore, FileBlobStore};
use kernel::Hrn;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::GraphTenant;

const ACCOUNT: &str = "acme";
const OTHER: &str = "other";
const ACTION: &str = "Artifacts::Action::\"IngestExecutionReceipt\"";
/// What the real pipeline prints and must never reach a receipt or the audit.
const SECRET: &str = "ghp_RealLookingSecretTokenValue0123456789";
const REVISION: &str = "0123456789abcdef0123456789abcdef01234567";

/// Without the harness the suite skips, unless the run demands it.
fn harness_available() -> bool {
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

/// Tokens are `<user>@<account>`; the service derives the account from the
/// verified principal.
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

fn cedar(policy: &str) -> Arc<dyn AuthorizerPort> {
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("the policy parses"),
    )))
}

/// alice: publishes, reads, links receipts and reads capsules. bob can read and
/// publish but does NOT hold `IngestExecutionReceipt`. mallory belongs to
/// another account and holds everything in hers.
fn policy() -> String {
    format!(
        r#"
permit(principal == Iam::User::"hrn:hodei:iam::{ACCOUNT}:User/alice",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"IngestExecutionReceipt",
                  Artifacts::Action::"ReadCapsule"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{ACCOUNT}:User/bob",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::{OTHER}:User/mallory",
       action in [Artifacts::Action::"UploadAsset", Artifacts::Action::"DownloadAsset",
                  Artifacts::Action::"IngestExecutionReceipt",
                  Artifacts::Action::"ReadCapsule"],
       resource);
"#
    )
}

struct Vertical {
    app: Router,
    store: Arc<DgraphCatalogStore>,
    evidence: DgraphEvidenceStore,
    cas: Arc<FileBlobStore>,
    gc: Arc<GcService>,
    audit: Arc<InMemoryAuditSink>,
    #[allow(dead_code)]
    tmp: TempDir,
    #[allow(dead_code)]
    tenant: GraphTenant,
}

async fn mount() -> Option<Vertical> {
    mount_with(None).await
}

async fn mount_with(limits: Option<ReceiptLimits>) -> Option<Vertical> {
    if !harness_available() {
        return None;
    }
    let tenant = GraphTenant::create();
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.raw_api_enabled = true;
    config.security.admin_api_enabled = true;
    tenant.apply(&mut config, &tmp.path().join("bootstrap-cas"));
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap must succeed");

    let trust = Arc::new(tenant.trust_store().await);
    let store = Arc::new(
        tenant
            .catalog_store()
            .await
            .with_trust_ledger(trust.clone()),
    );
    let reader: Arc<dyn CatalogReader> = store.clone();
    let writer: Arc<dyn CatalogWriter> = store.clone();
    let cas = Arc::new(FileBlobStore::new(tmp.path().join("cas")).expect("the CAS"));
    let ledger: Arc<dyn ContentLedger> = store.clone();
    let gc = Arc::new(
        GcService::new(ledger, cas.clone(), std::time::Duration::ZERO).expect("the GC service"),
    );
    let audit = Arc::new(InMemoryAuditSink::new());
    let content: Arc<dyn BlobStore> = cas.clone();
    let state = match limits {
        Some(limits) => state.with_receipt_limits(limits),
        None => state,
    };
    let state = state
        .with_catalog(reader, writer)
        .with_trust_ledger(trust.clone())
        .with_content_store(content)
        .with_gc(gc.clone())
        .with_audit_sink(audit.clone())
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(&policy()));
    let evidence = DgraphEvidenceStore::open(tenant.client().await)
        .await
        .expect("the evidence store opens");
    Some(Vertical {
        app: build_router(state, &config),
        store,
        evidence,
        cas,
        gc,
        audit,
        tmp,
        tenant,
    })
}

async fn seed(v: &Vertical, account: &str, key: &str) {
    v.store
        .create_repository(
            account,
            RepositoryDescriptor {
                hrn: repository_hrn("hodei", "artifacts", "eu", account, key),
                account: account.to_string(),
                repo_key: key.to_string(),
                format: RepositoryFormat::Raw,
                mode: RepositoryMode::Hosted,
                config: RepositoryConfig::new(DigestScope::Global),
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
        )
        .await
        .expect("the repository is seeded");
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    token: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if !body.is_empty() {
        builder = builder.header(header::CONTENT_TYPE, "application/json");
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .expect("a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, bytes.to_vec())
}

fn json_of(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

fn alice() -> String {
    format!("alice@{ACCOUNT}")
}

fn bob() -> String {
    format!("bob@{ACCOUNT}")
}

fn mallory() -> String {
    format!("mallory@{OTHER}")
}

async fn upload(v: &Vertical, account_token: &str, repo: &str, path: &str, bytes: &[u8]) {
    let (status, body) = send(
        &v.app,
        Method::PUT,
        &format!("/repository/{repo}/{path}"),
        Some(account_token),
        bytes.to_vec(),
    )
    .await;
    assert!(
        matches!(status, StatusCode::CREATED | StatusCode::OK),
        "{status}: {}",
        String::from_utf8_lossy(&body)
    );
}

/// The exact identity of an artifact, written from the spec of the name
/// (`artifact/<repository>/<algorithm>-<hex>` in the repository's region) and
/// not from the function the service uses.
fn artifact(account: &str, repository: &str, bytes: &[u8]) -> Hrn {
    let digest = ContentDigest::of_bytes(bytes);
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu".to_string(),
        account.to_string(),
        "artifact".to_string(),
        format!("{repository}/sha256-{}", digest.value),
    )
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

async fn post(
    v: &Vertical,
    token: Option<&str>,
    repo: &str,
    path: &str,
    receipt: &Value,
) -> (StatusCode, Value, Vec<u8>) {
    let body = json!({ "repository": repo, "path": path, "receipt": receipt });
    let (status, bytes) = send(
        &v.app,
        Method::POST,
        "/api/v1/execution-receipts",
        token,
        serde_json::to_vec(&body).unwrap(),
    )
    .await;
    (status, json_of(&bytes), bytes)
}

fn as_value(receipt: &ExecutionReceiptV1) -> Value {
    serde_json::to_value(receipt).unwrap()
}

/// The evidence ids the capsule of `subject` holds up under the builder
/// dimension, as the caller sees them over HTTP.
async fn builder_links(v: &Vertical, token: &str, subject: &Hrn) -> (StatusCode, Vec<String>) {
    let (status, bytes) = send(
        &v.app,
        Method::GET,
        &format!("/api/v1/capsules?hrn={}", encode(&subject.to_string())),
        Some(token),
        Vec::new(),
    )
    .await;
    let capsule = json_of(&bytes);
    let mut links: Vec<String> = capsule["profile"]["dimensions"]
        .as_array()
        .map(|dimensions| {
            dimensions
                .iter()
                .filter(|dimension| dimension["name"] == "builder_trust")
                .flat_map(|dimension| {
                    dimension["evidence"]
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                })
                .filter_map(|link| link["evidence_id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    links.sort();
    (status, links)
}

async fn stored(v: &Vertical, account: &str, subject: &Hrn) -> usize {
    v.evidence
        .evidence_of_subject(account, subject)
        .await
        .expect("the evidence store answers")
        .len()
}

fn audit_results(v: &Vertical) -> Vec<String> {
    v.audit
        .events()
        .iter()
        .filter(|event| event.action == ACTION)
        .map(|event| format!("{:?}", event.result))
        .collect()
}

// ---------------------------------------------------------------------------
// The REAL PipelineK
// ---------------------------------------------------------------------------

/// One run of the installed `pipelinek`, and what its public surface said.
struct RealRun {
    run_id: String,
    version: String,
    events: String,
    cursor: String,
    console: String,
    succeeded: bool,
    artifact: Vec<u8>,
}

fn pipelinek_available() -> bool {
    Command::new("pipelinek").arg("version").output().is_ok()
}

fn run_pipelinek(work: &Path, tag: &str, artifact: &str, succeed: bool) -> RealRun {
    let script = work.join(format!("{tag}.pipeline.kts"));
    let out = work.join(format!("{tag}.bin"));
    let command = format!(
        "printf '%s' '{artifact}' > {} && echo token={SECRET}{}",
        out.display(),
        if succeed { "" } else { " && exit 1" }
    );
    std::fs::write(
        &script,
        format!("pipeline {{ stages {{ stage(\"build\") {{ sh(\"{command}\") }} }} }}\n"),
    )
    .expect("the script is written");
    let db = work.join(format!("{tag}.db"));
    let control = work.join(format!("{tag}.ctl"));
    let run = Command::new("pipelinek")
        .arg("run")
        .arg("--db")
        .arg(&db)
        .arg("--control-root")
        .arg(&control)
        .arg(&script)
        .current_dir(work)
        .output()
        .expect("pipelinek runs");
    let console = format!(
        "{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let launch = console
        .lines()
        .find(|line| line.contains("\"pipelinek.launch/1\""))
        .unwrap_or_else(|| panic!("the run announces itself: {console}"));
    let run_id = json_of(launch.as_bytes())["runId"]
        .as_str()
        .expect("the launch names the run")
        .to_string();
    let events = Command::new("pipelinek")
        .arg("events")
        .arg("--db")
        .arg(&db)
        .arg(&run_id)
        .arg("--typed")
        .output()
        .expect("pipelinek reads the events");
    assert!(events.status.success(), "pipelinek events failed");
    let version = Command::new("pipelinek")
        .args(["version", "--json"])
        .output()
        .expect("pipelinek reports its version");
    let version = json_of(&version.stdout)["productVersion"]
        .as_str()
        .expect("a product version")
        .to_string();
    RealRun {
        run_id,
        version,
        events: String::from_utf8_lossy(&events.stdout).to_string(),
        cursor: String::from_utf8_lossy(&events.stderr).to_string(),
        console,
        succeeded: run.status.success(),
        artifact: std::fs::read(&out).expect("the pipeline built the artifact"),
    }
}

struct Runs {
    a: RealRun,
    b: RealRun,
    failed: RealRun,
}

/// Three real runs, made once: two that build the same bytes and one that fails.
fn runs() -> &'static Runs {
    static RUNS: std::sync::OnceLock<Runs> = std::sync::OnceLock::new();
    RUNS.get_or_init(|| {
        assert!(
            pipelinek_available(),
            "the real PipelineK ({PINNED_PIPELINEK_VERSION}) is not installed: this suite does not fake one"
        );
        let work = tempfile::tempdir().expect("work dir").keep();
        let artifact = "build output of the real pipeline, run twice";
        Runs {
            a: run_pipelinek(&work, "a", artifact, true),
            b: run_pipelinek(&work, "b", artifact, true),
            failed: run_pipelinek(&work, "failed", "never trusted", false),
        }
    })
}

/// The receipt the adapter exports from a real run for the bytes it built.
fn export(run: &RealRun) -> ExecutionReceiptV1 {
    export_with(run, &ContentDigest::of_bytes(&run.artifact).to_string())
}

fn export_with(run: &RealRun, digest: &str) -> ExecutionReceiptV1 {
    export_from_pipelinek(PipelinekExport {
        pipelinek_version: &run.version,
        events_jsonl: &run.events,
        cursor: &run.cursor,
        subject_digest: digest,
        source_revision: REVISION,
    })
    .expect("the adapter exports the real run")
}

// ---------------------------------------------------------------------------
// Cases
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_real_pipelinek_run_is_exported_and_linked_by_exact_identity() {
    let Some(v) = mount().await else { return };
    let run = &runs().a;
    // The adapter is pinned: the real PipelineK that ran is the one it accepts.
    assert_eq!(run.version, PINNED_PIPELINEK_VERSION);
    assert!(
        run.succeeded && run.console.contains(SECRET),
        "the real run printed its secret"
    );

    seed(&v, ACCOUNT, "builds").await;
    upload(&v, &alice(), "builds", "app/service.bin", &run.artifact).await;
    upload(&v, &alice(), "builds", "app/other.bin", b"another artifact").await;
    let receipt = export(run);
    assert_eq!(
        receipt.run_id, run.run_id,
        "the run id is the one PipelineK minted"
    );

    let (status, answer, raw) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&receipt),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&raw)
    );
    let subject = artifact(ACCOUNT, "builds", &run.artifact);
    assert_eq!(answer["result"], "created");
    assert_eq!(answer["subject"], subject.to_string());
    assert_eq!(answer["run_id"], run.run_id);
    assert_eq!(answer["source_revision"], REVISION);
    assert_eq!(
        answer["digest"],
        ContentDigest::of_bytes(&run.artifact).to_string()
    );
    let evidence_id = answer["evidence_id"].as_str().unwrap().to_string();

    // The link is by exact identity: the capsule of THAT artifact shows it...
    let (status, links) = builder_links(&v, &alice(), &subject).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(links, vec![evidence_id.clone()]);
    // ...and the capsule of another artifact of the same repository does not.
    let other = artifact(ACCOUNT, "builds", b"another artifact");
    assert_eq!(
        builder_links(&v, &alice(), &other).await.1,
        Vec::<String>::new()
    );
    assert_eq!(stored(&v, ACCOUNT, &other).await, 0);

    // The record keeps the receipt it admitted, byte for byte, in the CAS.
    let records = v
        .evidence
        .evidence_of_subject(ACCOUNT, &subject)
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    let kept = v
        .cas
        .get(&records[0].content().digest)
        .await
        .expect("the receipt content is kept");
    let kept: ExecutionReceiptV1 = serde_json::from_slice(&kept).unwrap();
    assert_eq!(kept, receipt);
    assert_eq!(records[0].producer().id, "pipelinek");
    assert_eq!(
        records[0].producer().version.as_deref(),
        Some(PINNED_PIPELINEK_VERSION)
    );

    // The collector does not take the evidence content: it is held.
    v.gc.sweep_platform("alice", ACCOUNT)
        .await
        .expect("a sweep");
    assert!(v.cas.exists(&records[0].content().digest).await.unwrap());
    assert_eq!(audit_results(&v), ["Allowed"]);
}

#[tokio::test]
async fn the_same_receipt_three_times_is_one_logical_receipt() {
    let Some(v) = mount().await else { return };
    let run = &runs().a;
    seed(&v, ACCOUNT, "builds").await;
    upload(&v, &alice(), "builds", "app/service.bin", &run.artifact).await;
    let receipt = as_value(&export(run));
    let subject = artifact(ACCOUNT, "builds", &run.artifact);

    let mut ids = Vec::new();
    let mut results = Vec::new();
    for _ in 0..3 {
        let (status, answer, raw) =
            post(&v, Some(&alice()), "builds", "app/service.bin", &receipt).await;
        assert!(
            status.is_success(),
            "{status} {}",
            String::from_utf8_lossy(&raw)
        );
        results.push((
            status.as_u16(),
            answer["result"].as_str().unwrap().to_string(),
        ));
        ids.push(answer["evidence_id"].as_str().unwrap().to_string());
    }
    assert_eq!(
        results,
        [
            (201, "created".to_string()),
            (200, "already_accepted".to_string()),
            (200, "already_accepted".to_string())
        ]
    );
    assert_eq!(ids[0], ids[1]);
    assert_eq!(ids[1], ids[2]);
    assert_eq!(
        stored(&v, ACCOUNT, &subject).await,
        1,
        "one logical receipt in the store"
    );
    assert_eq!(
        builder_links(&v, &alice(), &subject).await.1,
        vec![ids[0].clone()]
    );
}

#[tokio::test]
async fn two_runs_with_the_same_digest_are_both_linked_independently() {
    let Some(v) = mount().await else { return };
    let (a, b) = (&runs().a, &runs().b);
    assert_ne!(a.run_id, b.run_id, "two real runs, two run ids");
    assert_eq!(a.artifact, b.artifact, "both built the same bytes");
    seed(&v, ACCOUNT, "builds").await;
    upload(&v, &alice(), "builds", "app/service.bin", &a.artifact).await;
    let subject = artifact(ACCOUNT, "builds", &a.artifact);

    let (sa, answer_a, _) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&export(a)),
    )
    .await;
    let (sb, answer_b, _) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&export(b)),
    )
    .await;
    assert_eq!((sa, sb), (StatusCode::CREATED, StatusCode::CREATED));
    assert_eq!(answer_a["result"], "created");
    assert_eq!(answer_b["result"], "created");
    let (id_a, id_b) = (
        answer_a["evidence_id"].as_str().unwrap().to_string(),
        answer_b["evidence_id"].as_str().unwrap().to_string(),
    );
    assert_ne!(id_a, id_b, "independent references");

    let mut expected = vec![id_a.clone(), id_b.clone()];
    expected.sort();
    assert_eq!(builder_links(&v, &alice(), &subject).await.1, expected);
    let records = v
        .evidence
        .evidence_of_subject(ACCOUNT, &subject)
        .await
        .unwrap();
    assert_eq!(records.len(), 2);
    let mut run_ids = Vec::new();
    for record in &records {
        let kept = v.cas.get(&record.content().digest).await.unwrap();
        let kept: ExecutionReceiptV1 = serde_json::from_slice(&kept).unwrap();
        run_ids.push(kept.run_id);
    }
    run_ids.sort();
    let mut want = vec![a.run_id.clone(), b.run_id.clone()];
    want.sort();
    assert_eq!(run_ids, want, "each reference is its own run");
}

#[tokio::test]
async fn a_digest_that_is_not_the_bytes_is_rejected_and_publishes_no_lineage() {
    let Some(v) = mount().await else { return };
    let run = &runs().a;
    seed(&v, ACCOUNT, "builds").await;
    upload(&v, &alice(), "builds", "app/service.bin", &run.artifact).await;
    upload(
        &v,
        &alice(),
        "builds",
        "app/decoy.bin",
        b"a different artifact",
    )
    .await;
    let truth = artifact(ACCOUNT, "builds", &run.artifact);

    // A digest that nobody built.
    let forged = ContentDigest::of_bytes(b"tampered bytes").to_string();
    let (status, answer, raw) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&export_with(run, &forged)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        String::from_utf8_lossy(&raw)
    );
    assert_eq!(answer["error"], "E_RECEIPT_DIGEST_MISMATCH");
    // A digest that exists in the repository, but is another artifact's.
    let decoy = ContentDigest::of_bytes(b"a different artifact").to_string();
    let (status, answer, _) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&export_with(run, &decoy)),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(answer["error"], "E_RECEIPT_DIGEST_MISMATCH");

    // Nothing was published about any of the three identities.
    let identities = [
        truth.clone(),
        artifact(ACCOUNT, "builds", b"tampered bytes"),
        artifact(ACCOUNT, "builds", b"a different artifact"),
    ];
    for identity in &identities {
        assert_eq!(
            stored(&v, ACCOUNT, identity).await,
            0,
            "no evidence for {identity}"
        );
        assert_eq!(
            builder_links(&v, &alice(), identity).await.1,
            Vec::<String>::new(),
            "no lineage in the capsule of {identity}"
        );
    }
    assert_eq!(audit_results(&v), ["Denied", "Denied"]);

    // The control: the same real run with the TRUE digest links. The rejection
    // above was the digest check and nothing else.
    let (status, _, raw) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&export(run)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&raw)
    );
    assert_eq!(stored(&v, ACCOUNT, &truth).await, 1);
}

#[tokio::test]
async fn no_secret_reaches_a_receipt_the_answers_or_the_audit() {
    let Some(v) = mount().await else { return };
    let run = &runs().a;
    assert!(
        run.console.contains(SECRET),
        "the real pipeline printed the secret"
    );
    seed(&v, ACCOUNT, "builds").await;
    upload(&v, &alice(), "builds", "app/service.bin", &run.artifact).await;
    let receipt = export(run);
    let subject = artifact(ACCOUNT, "builds", &run.artifact);

    let mut seen = vec![serde_json::to_string(&receipt).unwrap(), run.events.clone()];
    let (status, _, raw) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&receipt),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    seen.push(String::from_utf8_lossy(&raw).to_string());

    // A receipt that carries the secret in a field of the contract...
    let mut hostile = as_value(&receipt);
    hostile["source_revision"] = json!(SECRET);
    let (status, _, raw) = post(&v, Some(&alice()), "builds", "app/service.bin", &hostile).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    seen.push(String::from_utf8_lossy(&raw).to_string());
    // ...and in a field the contract does not have.
    let mut smuggled = as_value(&receipt);
    smuggled["token"] = json!(SECRET);
    let (status, _, raw) = post(&v, Some(&alice()), "builds", "app/service.bin", &smuggled).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    seen.push(String::from_utf8_lossy(&raw).to_string());

    let records = v
        .evidence
        .evidence_of_subject(ACCOUNT, &subject)
        .await
        .unwrap();
    assert_eq!(records.len(), 1, "only the honest receipt was recorded");
    seen.push(
        String::from_utf8_lossy(&v.cas.get(&records[0].content().digest).await.unwrap())
            .to_string(),
    );
    seen.push(format!("{records:?}"));
    seen.push(format!("{:?}", v.audit.events()));
    let capsule = send(
        &v.app,
        Method::GET,
        &format!("/api/v1/capsules?hrn={}", encode(&subject.to_string())),
        Some(&alice()),
        Vec::new(),
    )
    .await;
    seen.push(String::from_utf8_lossy(&capsule.1).to_string());

    for text in &seen {
        assert!(
            !text.contains(SECRET),
            "a secret value leaked: {}",
            &text[..text.len().min(80)]
        );
    }
    assert_eq!(
        audit_results(&v),
        ["Allowed", "Denied"],
        "the refusal is audited, without the value"
    );
}

#[tokio::test]
async fn refusals_are_closed_and_accounts_are_isolated() {
    let Some(v) = mount().await else { return };
    let run = &runs().a;
    seed(&v, ACCOUNT, "builds").await;
    seed(&v, OTHER, "builds").await;
    upload(&v, &alice(), "builds", "app/service.bin", &run.artifact).await;
    let receipt = as_value(&export(run));
    let subject = artifact(ACCOUNT, "builds", &run.artifact);

    // No credential, and a principal without the dedicated action.
    let (status, _, _) = post(&v, None, "builds", "app/service.bin", &receipt).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _, _) = post(&v, Some(&bob()), "builds", "app/service.bin", &receipt).await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "uploading and downloading do not imply the action"
    );
    assert_eq!(audit_results(&v), ["Denied"]);

    // A run that failed does not export, and a receipt that says so is refused.
    let failed = &runs().failed;
    assert!(!failed.succeeded);
    assert!(
        export_from_pipelinek(PipelinekExport {
            pipelinek_version: &failed.version,
            events_jsonl: &failed.events,
            cursor: &failed.cursor,
            subject_digest: &ContentDigest::of_bytes(&run.artifact).to_string(),
            source_revision: REVISION,
        })
        .is_err()
    );
    let mut not_success = receipt.clone();
    not_success["outcome"] = json!("failure");
    let (status, answer, _) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &not_success,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(answer["error"], "E_RECEIPT_NOT_SUCCESS");
    // A producer version that is not the pinned one.
    let mut unpinned = receipt.clone();
    unpinned["producer"]["version"] = json!("0.98.0");
    let (status, answer, _) =
        post(&v, Some(&alice()), "builds", "app/service.bin", &unpinned).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(answer["error"], "E_RECEIPT_INVALID");
    // A finish time in the future, or before PipelineK could have existed.
    for when in ["2999-01-01T00:00:00Z", "1970-01-01T00:00:00Z"] {
        let mut bad_time = receipt.clone();
        bad_time["finished_at"] = json!(when);
        let (status, answer, _) =
            post(&v, Some(&alice()), "builds", "app/service.bin", &bad_time).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{when}");
        assert_eq!(answer["error"], "E_RECEIPT_INVALID");
    }
    // A path that names nothing: audited as a failure after Cedar.
    let (status, _, _) = post(&v, Some(&alice()), "builds", "app/missing.bin", &receipt).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        audit_results(&v),
        ["Denied", "Denied", "Denied", "Denied", "Denied", "Failed"],
        "every way out after Cedar is audited"
    );
    assert_eq!(
        stored(&v, ACCOUNT, &subject).await,
        0,
        "none of the refusals wrote"
    );

    // The owner links; the stranger cannot reach the owner's repository...
    let (status, _, _) = post(&v, Some(&alice()), "builds", "app/service.bin", &receipt).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, _, _) = post(&v, Some(&mallory()), "builds", "app/nothing.bin", &receipt).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "her own repository has no such asset"
    );
    // ...the same run id and digest in HER repository is her own record...
    upload(&v, &mallory(), "builds", "app/service.bin", &run.artifact).await;
    let (status, answer, _) =
        post(&v, Some(&mallory()), "builds", "app/service.bin", &receipt).await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "a new record, not the owner's: {answer}"
    );
    assert_eq!(answer["result"], "created");
    let hers = artifact(OTHER, "builds", &run.artifact);
    assert_eq!(answer["subject"], hers.to_string());
    // ...and nothing of the owner's changed, nor is it visible to her.
    assert_eq!(stored(&v, ACCOUNT, &subject).await, 1);
    assert_eq!(stored(&v, OTHER, &hers).await, 1);
    assert_eq!(stored(&v, OTHER, &subject).await, 0);
    assert_eq!(stored(&v, ACCOUNT, &hers).await, 0);
    let (status, _) = builder_links(&v, &mallory(), &subject).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(builder_links(&v, &alice(), &subject).await.1.len(), 1);
}

#[tokio::test]
async fn a_receipt_never_makes_the_builder_verified() {
    use hodei_evidence::CapsuleAssembler;
    use hodei_evidence::domain::capsule::{DimensionName, DimensionState};
    use hodei_evidence::{
        ContentRef, EvidenceId, EvidenceRecord, EvidenceType, EvidenceVerification, Normalization,
        Producer, SourceRef,
    };
    use hodei_trust_policy::{Requirement, RequirementKind, TrustContext, TrustPolicy, evaluate};

    let Some(v) = mount().await else { return };
    let run = &runs().a;
    seed(&v, ACCOUNT, "builds").await;
    upload(&v, &alice(), "builds", "app/service.bin", &run.artifact).await;
    let subject = artifact(ACCOUNT, "builds", &run.artifact);
    let (status, _, _) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&export(run)),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Over HTTP: the builder dimension holds the receipt as a declaration.
    let (_, bytes) = send(
        &v.app,
        Method::GET,
        &format!("/api/v1/capsules?hrn={}", encode(&subject.to_string())),
        Some(&alice()),
        Vec::new(),
    )
    .await;
    let capsule = json_of(&bytes);
    let builder = capsule["profile"]["dimensions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["name"] == "builder_trust")
        .expect("the builder dimension");
    assert_ne!(builder["state"], "VERIFIED");
    assert_eq!(builder["state"], "UNVERIFIED");
    assert_eq!(
        builder["evidence"][0]["verification"]["status"],
        "UNVERIFIED"
    );
    assert_eq!(
        builder["evidence"][0]["verification"]["method"],
        "exporter-declared"
    );

    // The promotion policy that demands a verified builder keeps denying.
    let policy = TrustPolicy::new(
        "needs-a-verified-builder",
        "1.0.0",
        vec![Requirement::denying(RequirementKind::Dimension {
            dimension: DimensionName::BuilderTrust,
            accepts: vec![DimensionState::Verified],
        })],
    );
    let records = v
        .evidence
        .evidence_of_subject(ACCOUNT, &subject)
        .await
        .unwrap();
    let real = CapsuleAssembler::new().assemble(&subject, &records, Utc::now());
    let decision = evaluate(&TrustContext::builder(&real).build(), &policy);
    assert!(
        !decision.advances(),
        "a receipt is not a verified builder: {decision:?}"
    );

    // The control: the same policy IS satisfiable by evidence that is verified,
    // so the denial above is the receipt's status and nothing else.
    let verified = EvidenceRecord::builder(EvidenceId::new("control"), subject.clone())
        .subjects(vec![subject.clone()])
        .evidence_type(EvidenceType::BuildMetadata)
        .source(SourceRef::by_external_id("control"))
        .content(ContentRef::by_digest(ContentDigest::of_bytes(b"control")))
        .observed_at(Utc::now())
        .recorded_at(Utc::now())
        .verification(EvidenceVerification::verified("signed-by-pinned-key"))
        .normalization(Normalization::new("control", "1.0.0"))
        .producer(Producer::new("connector", "pipelinek"))
        .build();
    let ok = CapsuleAssembler::new().assemble(&subject, &[verified], Utc::now());
    assert!(evaluate(&TrustContext::builder(&ok).build(), &policy).advances());
}

#[tokio::test]
async fn a_different_receipt_for_the_same_run_and_digest_is_a_conflict_not_a_replacement() {
    let Some(v) = mount().await else { return };
    let run = &runs().a;
    seed(&v, ACCOUNT, "builds").await;
    upload(&v, &alice(), "builds", "app/service.bin", &run.artifact).await;
    let subject = artifact(ACCOUNT, "builds", &run.artifact);
    let original = export(run);
    let (status, first, _) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&original),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // Same account, run and digest; a different declared revision.
    let mut other = original.clone();
    other.source_revision = "f".repeat(40);
    let (status, answer, _) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&other),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(answer["error"], "E_RECEIPT_CONFLICT");

    // The original is untouched and nothing else was recorded.
    let records = v
        .evidence
        .evidence_of_subject(ACCOUNT, &subject)
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    let kept: ExecutionReceiptV1 =
        serde_json::from_slice(&v.cas.get(&records[0].content().digest).await.unwrap()).unwrap();
    assert_eq!(kept, original);
    assert_eq!(builder_links(&v, &alice(), &subject).await.1.len(), 1);
    // The retention the refused attempt placed was released; the original's stays.
    let hold = |receipt: &ExecutionReceiptV1| {
        format!(
            "receipt-{}",
            ContentDigest::of_bytes(&receipt.canonical_bytes()).value
        )
    };
    assert!(v.gc.hold(&hold(&other)).await.unwrap().is_none());
    assert!(v.gc.hold(&hold(&original)).await.unwrap().is_some());
    assert_eq!(audit_results(&v), ["Allowed", "Denied"]);

    // The control: the same content again is the idempotent answer.
    let (status, again, _) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&original),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["evidence_id"], first["evidence_id"]);
}

#[tokio::test]
async fn the_admission_is_bounded_in_size_and_in_concurrency() {
    let run = &runs().a;
    // Size: the artifact is larger than what the service re-hashes.
    let Some(v) = mount_with(Some(ReceiptLimits::new(16, 4))).await else {
        return;
    };
    seed(&v, ACCOUNT, "builds").await;
    upload(&v, &alice(), "builds", "app/service.bin", &run.artifact).await;
    assert!(run.artifact.len() > 16);
    let (status, answer, _) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&export(run)),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(answer["error"], "E_ARTIFACT_TOO_LARGE");
    assert_eq!(
        stored(&v, ACCOUNT, &artifact(ACCOUNT, "builds", &run.artifact)).await,
        0
    );
    // The control: an artifact within the bound links under the same limits.
    let small = b"small artifact";
    upload(&v, &alice(), "builds", "app/small.bin", small).await;
    let digest = ContentDigest::of_bytes(small).to_string();
    let (status, _, raw) = post(
        &v,
        Some(&alice()),
        "builds",
        "app/small.bin",
        &as_value(&export_with(run, &digest)),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&raw)
    );
    assert_eq!(audit_results(&v), ["Denied", "Allowed"]);

    // Concurrency: no free slot is a 429 and nothing is read or written.
    let Some(busy) = mount_with(Some(ReceiptLimits::new(1 << 30, 0))).await else {
        return;
    };
    seed(&busy, ACCOUNT, "builds").await;
    upload(&busy, &alice(), "builds", "app/service.bin", &run.artifact).await;
    let (status, answer, _) = post(
        &busy,
        Some(&alice()),
        "builds",
        "app/service.bin",
        &as_value(&export(run)),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(answer["error"], "E_RECEIPT_BUSY");
    assert_eq!(
        stored(&busy, ACCOUNT, &artifact(ACCOUNT, "builds", &run.artifact)).await,
        0
    );
}
