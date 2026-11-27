//! HSP-321 tramo k (HSP-160 exposed): `POST /api/v1/passports/verify`, through
//! the real router over a real Dgraph namespace, the real Cedar engine, the
//! real trust ledger and passports exported by the real exporter.
//!
//! The roots of trust are the **operator's file**; every case below is a
//! behaviour of the route and each security control has a case that fails when
//! the control is removed:
//!
//! - a valid passport is valid; repeating it is the same verdict (pure
//!   verification, nothing consumed) and it grants nothing;
//! - an altered signature, a signer the operator does not pin, a signature on
//!   an algorithm outside the allow-list (SHA-224), a manifest that is not in
//!   canonical form, a tampered, missing or path-traversing blob, a document
//!   that is not a passport: all refused, never `valid`;
//! - a provenance signed by a key that is not a root, a root retired before /
//!   activated after the freeze, a revoked root, a signer outside its window or
//!   revoked, a passport written under another policy: refused;
//! - expired and not-yet-valid passports, with a bounded clock skew;
//! - a passport of another account, or of another repository, is refused
//!   before any verdict about it exists;
//! - a revoked or superseded promotion decision is refused; a revocation
//!   ledger that is down is a 503 (no verdict), never a pass;
//! - without the dedicated Cedar action there is no verification (403,
//!   audited) and the document is not even read; without roots it is a 501;
//! - the cost is bounded (document size, blob count);
//! - the response and the audit never carry keys or other accounts' data.
//!
//! Gated on `HODEI_GRAPH_IT_ENDPOINT` (with `HODEI_REQUIRE_IT=1` a missing
//! harness fails instead of skipping); serialize with `--test-threads=1`.

#![allow(dead_code)] // the shared scaffolding carries helpers other suites use

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use cedar_policy::Entities;
use chrono::{DateTime, Duration, Utc};
use hodei_artifacts_api::audit::AuditResult;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::passport_verify::PassportVerifier;
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::model::{
    DigestScope, PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog::{
    CatalogError, CatalogWriter, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt,
    ReceiptOutcome, TrustGrant, TrustLedger,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_evidence::standards::{RootRevision, TrustRoots, VerificationKey};
use hodei_hub::BlobStore;
use hodei_hub::blob::FileBlobStore;
use kernel::application::ports::clock::{FixedClock, Identity, UuidGenerator};
use serde_json::{Value, json};
use tower::ServiceExt;

use hodei_artifacts_api::bundles::passport::{
    PASSPORT_ARTIFACT_TYPE, PASSPORT_SIGNATURE_ARTIFACT_TYPE, PassportInputs, PassportPolicy,
    SignedDocument, export_passport,
};
use hodei_artifacts_api::bundles::{self, Mapping};
use hodei_artifacts_api::openpgp::MetadataSigner;

#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

const UPSTREAM_URL: &str = "https://repo.example.org/raw/";
const ISSUER: &str = "registry.example";
const ACTION: &str = "Artifacts::Action::\"VerifyTrustPassport\"";

include!("support/hsp118_site.rs");
include!("support/hsp160_material.rs");

// ------------------------------------------------------------------ fixture

fn hsp160_policy_for_it() -> PassportPolicy {
    hsp160_policy()
}

/// A package with a decided path, an undecided one and a withheld one.
async fn three_way_site() -> (Site, ContentDigest) {
    let site = Site::new().await;
    site.create_proxy("acme", "mirror", "central").await;
    site.create_proxy("acme", "second", "central").await;
    site.create_hosted("beta", "beta-repo").await;
    let decided = site
        .resolve("acme", "mirror", "tools/a.bin", b"tool a")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/a.bin",
        &decided,
        6,
        ReceiptOutcome::Fetched,
        "central",
        1,
    )
    .await;
    site.trust
        .grant(&[grant(&decided, "dec-1")])
        .await
        .expect("the grant exists");
    let bare = site
        .resolve("acme", "mirror", "tools/b.bin", b"tool b")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/b.bin",
        &bare,
        6,
        ReceiptOutcome::Fetched,
        "central",
        1,
    )
    .await;
    site.resolve("acme", "mirror", "tools/c.bin", b"tool c")
        .await;
    (site, decided)
}

fn grant(digest: &ContentDigest, decision: &str) -> TrustGrant {
    TrustGrant {
        account: "acme".to_string(),
        repository: "mirror".to_string(),
        digest: digest.clone(),
        decision_id: decision.to_string(),
        actor: "hrn:hodei:iam:acme:User/carol".to_string(),
        granted_at: Utc::now() - Duration::days(1),
        revoked_at: None,
        revoked_by: None,
        revoke_reason: None,
        subject: None,
        policy_version: Some("1.0.0".to_string()),
        capsule_hash: None,
        risk_version: None,
        valid_until: Some(Utc::now() + Duration::hours(2)),
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).expect("readable")).expect("JSON")
}

fn capsule_digest(capsule_dir: &Path) -> String {
    read_json(&capsule_dir.join("index.json"))["manifests"][0]["digest"]
        .as_str()
        .expect("a digest")
        .to_string()
}

struct World {
    site: Site,
    decided: ContentDigest,
    signer: MetadataSigner,
    alpha: SignedRoot,
    verified_at: DateTime<Utc>,
    bundle_dir: PathBuf,
    /// The good passport layout.
    layout: PathBuf,
}

async fn world() -> World {
    let (site, decided) = three_way_site().await;
    let signer = MetadataSigner::generate("HSP-321k <passport@example.local>").expect("a key");
    let bundle_dir = export_with(&site, "good", Some(&signer)).await;
    let verified_at = Utc::now();
    let alpha = SignedRoot::generate("hsp321k-root-alpha-v1", "alpha", ISSUER);
    let layout = export_passport_with(
        &bundle_dir,
        "good.passport",
        &alpha,
        &alpha,
        &signer,
        verified_at,
    );
    World {
        site,
        decided,
        signer,
        alpha,
        verified_at,
        bundle_dir,
        layout,
    }
}

/// Exports a passport whose provenance is signed by `signing` while the export
/// verifies under `published`'s roots.
fn export_passport_with(
    bundle_dir: &Path,
    name: &str,
    published: &SignedRoot,
    signing: &SignedRoot,
    signer: &MetadataSigner,
    verified_at: DateTime<Utc>,
) -> PathBuf {
    export_passport_under(
        bundle_dir,
        name,
        published,
        signing,
        signer,
        verified_at,
        hsp160_policy_for_it(),
    )
}

/// As [`export_passport_with`], under a given policy.
fn export_passport_under(
    bundle_dir: &Path,
    name: &str,
    published: &SignedRoot,
    signing: &SignedRoot,
    signer: &MetadataSigner,
    verified_at: DateTime<Utc>,
    policy: PassportPolicy,
) -> PathBuf {
    let capsule_dir = bundle_dir.join("oci");
    let digest = capsule_digest(&capsule_dir);
    let revision = published.revision(1, verified_at - Duration::days(30));
    let roots = published.trust_roots(revision.clone());
    let inputs = PassportInputs {
        bundle_dir,
        capsule_dir: &capsule_dir,
        capsule_image_digest: digest.clone(),
        provenance: hsp160_provenance(signing, &digest),
        policy,
        roots: &roots,
        published_revisions: vec![revision],
        verified_at,
    };
    let layout = bundle_dir.join(name);
    export_passport(&inputs, &layout, signer).expect("the passport exports");
    layout
}

// ------------------------------------------------------- layout manipulation

fn blob_file(layout: &Path, digest: &str) -> PathBuf {
    layout
        .join("blobs/sha256")
        .join(digest.strip_prefix("sha256:").expect("sha256"))
}

fn put_blob(layout: &Path, bytes: &[u8]) -> (String, u64) {
    let digest = ContentDigest::of_bytes(bytes).to_string();
    std::fs::write(blob_file(layout, &digest), bytes).expect("a blob is written");
    (digest, bytes.len() as u64)
}

fn copy_layout(from: &Path, to: &Path) {
    std::fs::create_dir_all(to.join("blobs/sha256")).unwrap();
    for name in ["oci-layout", "index.json"] {
        std::fs::copy(from.join(name), to.join(name)).unwrap();
    }
    for entry in std::fs::read_dir(from.join("blobs/sha256")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(
            entry.path(),
            to.join("blobs/sha256").join(entry.file_name()),
        )
        .unwrap();
    }
}

fn variant(world: &World, name: &str) -> PathBuf {
    let to = world.bundle_dir.join(name);
    copy_layout(&world.layout, &to);
    to
}

/// The index position, the manifest and its digest of an artifact type.
fn find_manifest(layout: &Path, artifact_type: &str) -> (usize, Value, String) {
    let index = read_json(&layout.join("index.json"));
    for (position, entry) in index["manifests"].as_array().unwrap().iter().enumerate() {
        let digest = entry["digest"].as_str().unwrap().to_string();
        let manifest: Value =
            serde_json::from_slice(&std::fs::read(blob_file(layout, &digest)).unwrap()).unwrap();
        if manifest["artifactType"] == artifact_type {
            return (position, manifest, digest);
        }
    }
    panic!("no manifest of {artifact_type}");
}

fn set_index_entry(layout: &Path, position: usize, digest: &str, size: u64) {
    let mut index = read_json(&layout.join("index.json"));
    index["manifests"][position]["digest"] = json!(digest);
    index["manifests"][position]["size"] = json!(size);
    std::fs::write(
        layout.join("index.json"),
        serde_json::to_vec_pretty(&index).unwrap(),
    )
    .unwrap();
}

/// Replaces the signature the layout carries with `(fingerprint, armor)`,
/// computed from the exact bytes the signature manifest points at.
fn rewrite_signature(layout: &Path, make: impl FnOnce(&[u8]) -> (String, String)) {
    let (position, mut manifest, _) = find_manifest(layout, PASSPORT_SIGNATURE_ARTIFACT_TYPE);
    let subject = manifest["subject"]["digest"].as_str().unwrap().to_string();
    let signed = std::fs::read(blob_file(layout, &subject)).unwrap();
    let (fingerprint, armor) = make(&signed);
    let document = serde_json::to_vec_pretty(&json!({
        "format": "openpgp-detached",
        "signer": fingerprint,
        "signature": armor,
    }))
    .unwrap();
    let (digest, size) = put_blob(layout, &document);
    manifest["layers"][0]["digest"] = json!(digest);
    manifest["layers"][0]["size"] = json!(size);
    let (manifest_digest, manifest_size) =
        put_blob(layout, &serde_json::to_vec_pretty(&manifest).unwrap());
    set_index_entry(layout, position, &manifest_digest, manifest_size);
}

/// Replaces the passport manifest by `bytes` and re-signs the new bytes with
/// the given signature maker, so only the form of the manifest changed.
fn rewrite_passport_manifest(
    layout: &Path,
    bytes: &[u8],
    make: impl FnOnce(&[u8]) -> (String, String),
) {
    let (position, _, _) = find_manifest(layout, PASSPORT_ARTIFACT_TYPE);
    let (digest, size) = put_blob(layout, bytes);
    set_index_entry(layout, position, &digest, size);
    let (sig_position, mut sig_manifest, _) =
        find_manifest(layout, PASSPORT_SIGNATURE_ARTIFACT_TYPE);
    sig_manifest["subject"]["digest"] = json!(digest);
    sig_manifest["subject"]["size"] = json!(size);
    let (d, s) = put_blob(layout, &serde_json::to_vec_pretty(&sig_manifest).unwrap());
    set_index_entry(layout, sig_position, &d, s);
    rewrite_signature(layout, make);
}

fn sign_with(signer: &MetadataSigner) -> impl FnOnce(&[u8]) -> (String, String) + '_ {
    move |bytes| {
        (
            signer.fingerprint_hex(),
            String::from_utf8(signer.sign_detached(bytes).unwrap()).unwrap(),
        )
    }
}

/// The passport layout as the document the route takes.
fn document_of(layout: &Path) -> Value {
    let mut blobs = Vec::new();
    for entry in std::fs::read_dir(layout.join("blobs/sha256")).unwrap() {
        let entry = entry.unwrap();
        blobs.push(json!({
            "digest": format!("sha256:{}", entry.file_name().to_string_lossy()),
            "data_base64": BASE64.encode(std::fs::read(entry.path()).unwrap()),
        }));
    }
    json!({
        "oci_layout": std::fs::read_to_string(layout.join("oci-layout")).unwrap(),
        "index": std::fs::read_to_string(layout.join("index.json")).unwrap(),
        "blobs": blobs,
    })
}

// ------------------------------------------------------------------- the app

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

/// alice (acme) and carol (beta) hold `VerifyTrustPassport`; bob (acme) can
/// download but does not.
fn policy() -> &'static str {
    r#"
permit(principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
       action in [Artifacts::Action::"VerifyTrustPassport", Artifacts::Action::"DownloadAsset"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::acme:User/bob",
       action in [Artifacts::Action::"DownloadAsset"],
       resource);
permit(principal == Iam::User::"hrn:hodei:iam::beta:User/carol",
       action in [Artifacts::Action::"VerifyTrustPassport", Artifacts::Action::"DownloadAsset"],
       resource);
"#
}

/// A ledger that cannot answer.
struct DownLedger;

fn down() -> CatalogError {
    CatalogError::Unavailable {
        detail: "the ledger is down".to_string(),
    }
}

#[async_trait]
impl TrustLedger for DownLedger {
    async fn grant(&self, _: &[TrustGrant]) -> Result<(), CatalogError> {
        Err(down())
    }
    async fn revoke_decision(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        Err(down())
    }
    async fn revoke_decision_for_account(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        Err(down())
    }
    async fn revoke_digest(
        &self,
        _: &str,
        _: &str,
        _: &ContentDigest,
        _: &str,
        _: &str,
        _: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        Err(down())
    }
    async fn active_grant(
        &self,
        _: &str,
        _: &str,
        _: &ContentDigest,
        _: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError> {
        Err(down())
    }
    async fn grants_of(&self, _: &str, _: &str) -> Result<Vec<TrustGrant>, CatalogError> {
        Err(down())
    }
}

/// A ledger that answers every read an hour from now.
struct SlowLedger;

#[async_trait]
impl TrustLedger for SlowLedger {
    async fn grant(&self, g: &[TrustGrant]) -> Result<(), CatalogError> {
        DownLedger.grant(g).await
    }
    async fn revoke_decision(
        &self,
        a: &str,
        b: &str,
        c: &str,
        d: &str,
        e: &str,
        f: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        DownLedger.revoke_decision(a, b, c, d, e, f).await
    }
    async fn revoke_decision_for_account(
        &self,
        a: &str,
        b: &str,
        c: &str,
        d: &str,
        e: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        DownLedger.revoke_decision_for_account(a, b, c, d, e).await
    }
    async fn revoke_digest(
        &self,
        a: &str,
        b: &str,
        c: &ContentDigest,
        d: &str,
        e: &str,
        f: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        DownLedger.revoke_digest(a, b, c, d, e, f).await
    }
    async fn active_grant(
        &self,
        _: &str,
        _: &str,
        _: &ContentDigest,
        _: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError> {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        Ok(None)
    }
    async fn grants_of(&self, a: &str, b: &str) -> Result<Vec<TrustGrant>, CatalogError> {
        DownLedger.grants_of(a, b).await
    }
}

#[derive(Default)]
struct Opts {
    /// The operator's file; `None` mounts no verifier (501).
    roots: Option<String>,
    ledger_down: bool,
    /// The moment the service believes it is, relative to the real one.
    at: Option<DateTime<Utc>>,
    /// Load the verifier through `trust.passport_verification_file`.
    through_file: bool,
    /// A ledger of the test's own (slow, broken...).
    ledger: Option<Arc<dyn TrustLedger>>,
}

struct Mounted {
    app: Router,
    audit: Arc<hodei_artifacts_api::audit::InMemoryAuditSink>,
}

fn roots_json(world: &World, tweak: impl FnOnce(&mut Value)) -> String {
    let mut file = json!({
        "version": 1,
        "policy": hsp160_policy_for_it(),
        "issuer": ISSUER,
        "max_age_secs": 86400,
        "clock_skew_secs": 60,
        "min_slsa_level": 1,
        "provenance_roots": [{
            "lineage": "alpha",
            "revision": 1,
            "key_id": world.alpha.key_id,
            "public_point_hex": world.alpha.point.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "issuer": ISSUER,
            "activated_at": (world.verified_at - Duration::days(30)).to_rfc3339(),
        }],
        "signers": [{
            "id": "release-signer",
            "public_key_armor": world.signer.key_asc().unwrap(),
            "valid_from": (world.verified_at - Duration::days(30)).to_rfc3339(),
        }],
    });
    tweak(&mut file);
    file.to_string()
}

async fn mount(world: &World, opts: Opts) -> Mounted {
    let mut config = AppConfig::default();
    world.site.tmp.apply(&mut config);
    config.security.raw_api_enabled = true;
    config.security.admin_api_enabled = true;
    // The strict profile of the revocation cache: every request asks the
    // shared authority (the default lease is 5 s, see the limitations).
    config.trust.revocation_verify_secs = 0;
    let keep = tempfile::tempdir().expect("temp dir");
    if opts.through_file
        && let Some(text) = &opts.roots
    {
        let path = keep.path().join("passport-roots.json");
        std::fs::write(&path, text).unwrap();
        config.trust.passport_verification_file = Some(path.to_string_lossy().into_owned());
    }
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
    let audit = Arc::new(hodei_artifacts_api::audit::InMemoryAuditSink::new());
    let mut state = state
        .with_audit_sink(audit.clone())
        .with_authenticator(Arc::new(TokenAuthenticator))
        .with_authorization(cedar(policy()))
        .with_identity(Identity::new(
            Arc::new(FixedClock::new(opts.at.unwrap_or_else(Utc::now))),
            Arc::new(UuidGenerator),
        ));
    if !opts.through_file
        && let Some(text) = &opts.roots
    {
        state = state.with_passport_verifier(Arc::new(
            PassportVerifier::from_json(text).expect("the operator's file is valid"),
        ));
    }
    if opts.ledger_down {
        state = state.with_trust_ledger(Arc::new(DownLedger));
    }
    if let Some(ledger) = opts.ledger {
        state = state.with_trust_ledger(ledger);
    }
    std::mem::forget(keep);
    Mounted {
        app: build_router(state, &config),
        audit,
    }
}

async fn good(world: &World) -> Mounted {
    mount(
        world,
        Opts {
            roots: Some(roots_json(world, |_| {})),
            ..Opts::default()
        },
    )
    .await
}

async fn post(
    app: &Router,
    token: Option<&str>,
    repository: &str,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(format!("/api/v1/passports/verify?repository={repository}"))
        .header(header::HOST, "127.0.0.1")
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
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

async fn verify(
    app: &Router,
    token: &str,
    repository: &str,
    document: &Value,
) -> (StatusCode, Value) {
    let (status, bytes) = post(
        app,
        Some(token),
        repository,
        serde_json::to_vec(document).unwrap(),
    )
    .await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn results(mounted: &Mounted) -> Vec<AuditResult> {
    mounted
        .audit
        .events()
        .iter()
        .filter(|event| event.action == ACTION)
        .map(|event| event.result)
        .collect()
}

/// The verdict must be a refusal with this code, and never `valid`.
#[track_caller]
fn assert_refused(outcome: &(StatusCode, Value), code: &str) {
    assert_ne!(
        outcome.1["verdict"], "valid",
        "a refusal is never valid: {outcome:?}"
    );
    assert_eq!(outcome.1["code"], code, "{outcome:?}");
    assert!(
        outcome.0 == StatusCode::UNPROCESSABLE_ENTITY || outcome.0 == StatusCode::PAYLOAD_TOO_LARGE,
        "{outcome:?}"
    );
    assert_eq!(outcome.1["grants_conferred"], false);
}

// --------------------------------------------------------------------- cases

#[tokio::test]
async fn a_valid_passport_is_valid_idempotent_audited_and_grants_nothing() {
    let world = world().await;
    let mounted = good(&world).await;
    let document = document_of(&world.layout);

    let (status, body) = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["verdict"], "valid");
    assert_eq!(body["grants_conferred"], false);
    assert_eq!(body["repository"], "mirror");
    assert_eq!(body["revocation"]["checked"], "ledger");
    assert_eq!(body["revocation"]["decisions"], 1);
    assert_eq!(body["root"]["lineage"], "alpha");
    eprintln!("slsa_result = {}", body["slsa_result"]);

    // Pure verification: the same document gives the same verdict, again.
    let (again, repeated) = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_eq!(again, StatusCode::OK);
    assert_eq!(repeated, body);
    assert_eq!(
        results(&mounted),
        vec![AuditResult::Allowed, AuditResult::Allowed]
    );

    // Nothing secret leaves in the answer or in the audit trail.
    let text = body.to_string();
    assert!(
        !text.contains("BEGIN PGP"),
        "no key material in the verdict"
    );
    let point: String = world
        .alpha
        .point
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert!(!text.contains(&point), "no root key in the verdict");
    for event in mounted.audit.events() {
        let line = format!("{event:?}");
        assert!(
            !line.contains("BEGIN PGP") && !line.contains(&point),
            "{line}"
        );
    }

    // A verdict is not a credential: presenting it (or the passport) to an
    // authorization path buys nothing.
    let (status, _) = post(
        &mounted.app,
        None,
        "mirror",
        serde_json::to_vec(&document).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn the_route_is_closed_without_roots_without_the_action_and_without_credentials() {
    let world = world().await;
    let document = document_of(&world.layout);

    // No roots pinned: 501, and never "verified".
    let none = mount(&world, Opts::default()).await;
    let (status, _) = verify(&none.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    assert_eq!(results(&none), vec![AuditResult::Failed]);

    // bob can download but does not hold VerifyTrustPassport: 403, audited,
    // and the body is not even read (garbage still gets the 403).
    let mounted = good(&world).await;
    let (status, bytes) = post(
        &mounted.app,
        Some("bob@acme"),
        "mirror",
        b"not json".to_vec(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(results(&mounted), vec![AuditResult::Denied]);

    // Without credentials: 401.
    let (status, _) = post(
        &mounted.app,
        None,
        "mirror",
        serde_json::to_vec(&document).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // The repository is required and must be the caller's.
    let (status, _) = verify(&mounted.app, "alice@acme", "does-not-exist", &document).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_operators_file_is_the_only_root_and_a_bad_file_stops_the_boot() {
    let world = world().await;
    let document = document_of(&world.layout);
    // The deployment contract: `trust.passport_verification_file`.
    let through_file = mount(
        &world,
        Opts {
            roots: Some(roots_json(&world, |_| {})),
            through_file: true,
            ..Opts::default()
        },
    )
    .await;
    let (status, body) = verify(&through_file.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // A declared file that is not usable stops the boot (fail closed).
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("broken.json");
    std::fs::write(&path, "{ not json").unwrap();
    let mut config = AppConfig::default();
    world.site.tmp.apply(&mut config);
    config.trust.passport_verification_file = Some(path.to_string_lossy().into_owned());
    let booted = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await;
    assert!(
        booted.is_err(),
        "a declared but unusable file must stop the boot"
    );

    // The passport's own trust-roots layer (public documentation of the
    // exporter) does not make a root: a verifier that pins another provenance
    // root refuses a passport signed by `alpha`.
    let other = SignedRoot::generate("hsp321k-other", "other", ISSUER);
    let elsewhere = mount(
        &world,
        Opts {
            roots: Some(roots_json(&world, |file| {
                file["provenance_roots"][0]["key_id"] = json!(other.key_id);
                file["provenance_roots"][0]["lineage"] = json!("other");
                file["provenance_roots"][0]["public_point_hex"] = json!(
                    other
                        .point
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>()
                );
            })),
            ..Opts::default()
        },
    )
    .await;
    let outcome = verify(&elsewhere.app, "alice@acme", "mirror", &document).await;
    assert_refused(&outcome, "verdict-mismatch");
}

#[tokio::test]
async fn signatures_algorithms_canonical_form_and_blobs_are_all_checked() {
    let world = world().await;
    let mounted = good(&world).await;
    let attempt = |layout: &Path| document_of(layout);

    // A signer the operator does not pin.
    let stranger = MetadataSigner::generate("stranger <s@example.local>").unwrap();
    let unpinned = variant(&world, "unpinned");
    rewrite_signature(&unpinned, sign_with(&stranger));
    let outcome = verify(&mounted.app, "alice@acme", "mirror", &attempt(&unpinned)).await;
    assert_refused(&outcome, "signature-invalid");

    // The stranger claims the pinned fingerprint: the key that verifies is the
    // pinned one, so the signature does not hold.
    let impostor = variant(&world, "impostor");
    let pinned_fp = world.signer.fingerprint_hex();
    rewrite_signature(&impostor, |bytes| {
        (
            pinned_fp.clone(),
            String::from_utf8(stranger.sign_detached(bytes).unwrap()).unwrap(),
        )
    });
    assert_refused(
        &verify(&mounted.app, "alice@acme", "mirror", &attempt(&impostor)).await,
        "signature-invalid",
    );

    // The pinned key signs OTHER bytes: an altered signature.
    let altered = variant(&world, "altered");
    rewrite_signature(&altered, |bytes| {
        let mut other = bytes.to_vec();
        other.push(b' ');
        (
            world.signer.fingerprint_hex(),
            String::from_utf8(world.signer.sign_detached(&other).unwrap()).unwrap(),
        )
    });
    assert_refused(
        &verify(&mounted.app, "alice@acme", "mirror", &attempt(&altered)).await,
        "signature-invalid",
    );

    // Same manifest, validly signed by the pinned key, but not in canonical
    // form (compact JSON instead of the writer's): signatures are over
    // canonical bytes only.
    let compact = variant(&world, "compact");
    let (_, _, digest) = find_manifest(&compact, PASSPORT_ARTIFACT_TYPE);
    let parsed: Value =
        serde_json::from_slice(&std::fs::read(blob_file(&compact, &digest)).unwrap()).unwrap();
    rewrite_passport_manifest(
        &compact,
        &serde_json::to_vec(&parsed).unwrap(),
        sign_with(&world.signer),
    );
    assert_refused(
        &verify(&mounted.app, "alice@acme", "mirror", &attempt(&compact)).await,
        "malformed",
    );

    // A blob whose bytes are not its digest.
    let tampered = variant(&world, "tampered");
    let (_, manifest, _) = find_manifest(&tampered, PASSPORT_ARTIFACT_TYPE);
    let vsa = manifest["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["mediaType"] == "application/vnd.slsa.verification_summary.v1+json")
        .unwrap()["digest"]
        .as_str()
        .unwrap()
        .to_string();
    let mut bytes = std::fs::read(blob_file(&tampered, &vsa)).unwrap();
    bytes.push(b' ');
    std::fs::write(blob_file(&tampered, &vsa), bytes).unwrap();
    assert_refused(
        &verify(&mounted.app, "alice@acme", "mirror", &attempt(&tampered)).await,
        "tampered-blob",
    );

    // A blob the manifest declares is not in the document.
    let missing = variant(&world, "missing");
    std::fs::remove_file(blob_file(&missing, &vsa)).unwrap();
    assert_refused(
        &verify(&mounted.app, "alice@acme", "mirror", &attempt(&missing)).await,
        "missing-evidence",
    );

    // A digest that tries to steer a file read out of the layout. The answer is
    // a refusal, never a read of the host file system, never a panic.
    let mut document = attempt(&world.layout);
    let mut index: Value = serde_json::from_str(document["index"].as_str().unwrap()).unwrap();
    index["manifests"][0]["digest"] = json!("sha256:../../../../../../etc/passwd");
    document["index"] = json!(index.to_string());
    let outcome = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_ne!(outcome.1["verdict"], "valid");
    assert_eq!(outcome.0, StatusCode::UNPROCESSABLE_ENTITY, "{outcome:?}");
    let mut document = attempt(&world.layout);
    document["blobs"][0]["digest"] = json!("sha256:../../x");
    assert_refused(
        &verify(&mounted.app, "alice@acme", "mirror", &document).await,
        "malformed",
    );

    // Not a passport at all; and unknown members are not tolerated.
    let mut garbage = attempt(&world.layout);
    garbage["index"] = json!("{ nope");
    assert_refused(
        &verify(&mounted.app, "alice@acme", "mirror", &garbage).await,
        "malformed",
    );
    let mut extra = attempt(&world.layout);
    extra["roots"] = json!("sha256:smuggled");
    let (status, _) = verify(&mounted.app, "alice@acme", "mirror", &extra).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // A duplicate blob is refused, not merged.
    let mut duplicate = attempt(&world.layout);
    let first = duplicate["blobs"][0].clone();
    duplicate["blobs"].as_array_mut().unwrap().push(first);
    assert_refused(
        &verify(&mounted.app, "alice@acme", "mirror", &duplicate).await,
        "malformed",
    );

    // The untouched passport is still valid: every refusal above is the
    // control's, not an accident of the fixture.
    let (status, body) = verify(
        &mounted.app,
        "alice@acme",
        "mirror",
        &attempt(&world.layout),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        results(&mounted)
            .iter()
            .filter(|r| **r == AuditResult::Allowed)
            .count()
            == 1
            && results(&mounted)
                .iter()
                .filter(|r| **r == AuditResult::Denied)
                .count()
                >= 10
    );
}

#[tokio::test]
async fn roots_windows_policy_and_the_provenance_key_are_the_operators() {
    let world = world().await;
    let document = document_of(&world.layout);
    let mount_with = |tweak: Box<dyn FnOnce(&mut Value)>| {
        let text = roots_json(&world, tweak);
        mount(
            &world,
            Opts {
                roots: Some(text),
                ..Opts::default()
            },
        )
    };

    // Provenance signed by a key that is not a root: the record honestly says
    // FAILED and the route does not call it valid.
    let beta = SignedRoot::generate("hsp321k-beta", "beta", "other-registry.example");
    let foreign = export_passport_with(
        &world.bundle_dir,
        "foreign.passport",
        &world.alpha,
        &beta,
        &world.signer,
        world.verified_at,
    );
    let mounted = good(&world).await;
    assert_refused(
        &verify(&mounted.app, "alice@acme", "mirror", &document_of(&foreign)).await,
        "verification-failed",
    );

    // A root retired before the passport was frozen.
    let v = verified(&world);
    let retired = mount_with(Box::new(move |file| {
        file["provenance_roots"][0]["retired_at"] = json!((v - Duration::days(1)).to_rfc3339());
    }))
    .await;
    assert_refused(
        &verify(&retired.app, "alice@acme", "mirror", &document).await,
        "root-not-valid",
    );

    // A root activated only after the passport was frozen.
    let late = mount_with(Box::new(move |file| {
        file["provenance_roots"][0]["activated_at"] = json!((v + Duration::hours(1)).to_rfc3339());
    }))
    .await;
    assert_refused(
        &verify(&late.app, "alice@acme", "mirror", &document).await,
        "root-not-valid",
    );

    // A revoked root: the signature holds, the trust is taken back.
    let revoked = mount_with(Box::new(move |file| {
        file["provenance_roots"][0]["revoked_at"] = json!((v - Duration::hours(1)).to_rfc3339());
    }))
    .await;
    assert_refused(
        &verify(&revoked.app, "alice@acme", "mirror", &document).await,
        "signer-revoked",
    );

    // A rotated lineage keeps verifying what its retired revision froze while
    // it was active, as long as the revision is still inside its window by the
    // server's clock (rotation is not revocation).
    let next = SignedRoot::generate("hsp321k-alpha-v2", "alpha", ISSUER);
    let next_point: String = next.point.iter().map(|b| format!("{b:02x}")).collect();
    let rotate = {
        let next_point = next_point.clone();
        move |historical: bool| {
            let next_point = next_point.clone();
            Box::new(move |file: &mut Value| {
                file["provenance_roots"][0]["retired_at"] =
                    json!((v + Duration::minutes(5)).to_rfc3339());
                file["provenance_roots"][0]["historical_ok"] = json!(historical);
                file["provenance_roots"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({
                        "lineage": "alpha", "revision": 2, "key_id": "hsp321k-alpha-v2",
                        "public_point_hex": next_point,
                        "issuer": ISSUER, "activated_at": (v + Duration::minutes(5)).to_rfc3339(),
                    }));
            }) as Box<dyn FnOnce(&mut Value)>
        }
    };
    let mount_at = |at: DateTime<Utc>, tweak: Box<dyn FnOnce(&mut Value)>| {
        let text = roots_json(&world, tweak);
        mount(
            &world,
            Opts {
                roots: Some(text),
                at: Some(at),
                ..Opts::default()
            },
        )
    };
    let rotated = mount_at(v + Duration::minutes(1), rotate(false)).await;
    let (status, body) = verify(&rotated.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // H3: the freeze time is the signer's own claim. Once the revision is
    // retired by the server's clock it no longer vouches for it, even for a
    // freeze dated inside its window ...
    let after = v + Duration::minutes(10);
    let retired_now = mount_at(after, rotate(false)).await;
    assert_refused(
        &verify(&retired_now.app, "alice@acme", "mirror", &document).await,
        "root-not-valid",
    );
    // ... unless the operator keeps the revision `historical_ok`.
    let historical = mount_at(after, rotate(true)).await;
    let (status, body) = verify(&historical.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    // The same for the passport signer's `valid_until`.
    let signer_closed = mount_at(
        after,
        Box::new(move |file| {
            file["signers"][0]["valid_until"] = json!((v + Duration::minutes(5)).to_rfc3339());
        }),
    )
    .await;
    assert_refused(
        &verify(&signer_closed.app, "alice@acme", "mirror", &document).await,
        "signer-not-valid",
    );
    let signer_historical = mount_at(
        after,
        Box::new(move |file| {
            file["signers"][0]["valid_until"] = json!((v + Duration::minutes(5)).to_rfc3339());
            file["signers"][0]["historical_ok"] = json!(true);
        }),
    )
    .await;
    let (status, body) = verify(&signer_historical.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The passport signer outside its window, and revoked.
    let expired_signer = mount_with(Box::new(move |file| {
        file["signers"][0]["valid_until"] = json!((v - Duration::hours(1)).to_rfc3339());
    }))
    .await;
    assert_refused(
        &verify(&expired_signer.app, "alice@acme", "mirror", &document).await,
        "signer-not-valid",
    );
    let not_yet = mount_with(Box::new(move |file| {
        file["signers"][0]["valid_from"] = json!((v + Duration::hours(1)).to_rfc3339());
    }))
    .await;
    assert_refused(
        &verify(&not_yet.app, "alice@acme", "mirror", &document).await,
        "signer-not-valid",
    );
    let revoked_signer = mount_with(Box::new(move |file| {
        file["signers"][0]["revoked_at"] = json!((v - Duration::minutes(1)).to_rfc3339());
    }))
    .await;
    assert_refused(
        &verify(&revoked_signer.app, "alice@acme", "mirror", &document).await,
        "signer-not-valid",
    );

    // A policy other than the one the passport was written under.
    let other_policy = mount_with(Box::new(|file| {
        file["policy"]["version"] = json!("2");
    }))
    .await;
    assert_refused(
        &verify(&other_policy.app, "alice@acme", "mirror", &document).await,
        "policy-mismatch",
    );

    // An issuer pin the roots do not satisfy.
    let pinned = mount_with(Box::new(|file| {
        file["issuer"] = json!("someone-else.example");
    }))
    .await;
    assert_refused(
        &verify(&pinned.app, "alice@acme", "mirror", &document).await,
        "verification-failed",
    );

    // The signing key of the passport is not interchangeable with another
    // pinned signer: pin only a different certificate.
    let other = MetadataSigner::generate("other <o@example.local>").unwrap();
    let wrong_signer = mount_with(Box::new(move |file| {
        file["signers"][0]["public_key_armor"] = json!(other.key_asc().unwrap());
    }))
    .await;
    assert_refused(
        &verify(&wrong_signer.app, "alice@acme", "mirror", &document).await,
        "signature-invalid",
    );
}

fn verified(world: &World) -> DateTime<Utc> {
    world.verified_at
}

#[tokio::test]
async fn expired_and_not_yet_valid_passports_are_refused_with_a_bounded_skew() {
    let world = world().await;
    let document = document_of(&world.layout);
    let roots = roots_json(&world, |_| {});

    // Two days later than the freeze (max age one day): expired.
    let late = mount(
        &world,
        Opts {
            roots: Some(roots.clone()),
            at: Some(world.verified_at + Duration::days(2)),
            ..Opts::default()
        },
    )
    .await;
    assert_refused(
        &verify(&late.app, "alice@acme", "mirror", &document).await,
        "expired",
    );

    // The service believes it is 2 minutes before the freeze: the passport is
    // dated after the present moment (past the 60 s skew).
    let early = mount(
        &world,
        Opts {
            roots: Some(roots.clone()),
            at: Some(world.verified_at - Duration::minutes(2)),
            ..Opts::default()
        },
    )
    .await;
    assert_refused(
        &verify(&early.app, "alice@acme", "mirror", &document).await,
        "not-yet-valid",
    );

    // Within the skew (30 s ahead of the clock) it is still valid.
    let within = mount(
        &world,
        Opts {
            roots: Some(roots),
            at: Some(world.verified_at - Duration::seconds(30)),
            ..Opts::default()
        },
    )
    .await;
    let (status, body) = verify(&within.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The skew itself is bounded by the operator's file: more than the ceiling
    // does not even boot.
    let too_much = roots_json(&world, |file| file["clock_skew_secs"] = json!(3600));
    assert!(PassportVerifier::from_json(&too_much).is_err());
}

#[tokio::test]
async fn a_passport_of_another_account_or_repository_is_refused_before_any_verdict() {
    let world = world().await;
    let mounted = good(&world).await;
    let document = document_of(&world.layout);

    // carol (beta) holds the action in her own account and brings acme's
    // passport for her own repository: refused as not addressed to her, with
    // nothing about its validity and nothing about acme in the answer.
    let (status, bytes) = post(
        &mounted.app,
        Some("carol@beta"),
        "beta-repo",
        serde_json::to_vec(&document).unwrap(),
    )
    .await;
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["code"], "audience-mismatch");
    let text = String::from_utf8_lossy(&bytes);
    assert!(!text.contains("acme") && !text.contains("mirror"), "{text}");

    // Even a tampered acme passport gets the same answer from carol: there is
    // no oracle on the passports of others.
    let tampered = variant(&world, "tampered-for-carol");
    let (_, manifest, _) = find_manifest(&tampered, PASSPORT_ARTIFACT_TYPE);
    let vsa = manifest["layers"][0]["digest"]
        .as_str()
        .unwrap()
        .to_string();
    let mut raw = std::fs::read(blob_file(&tampered, &vsa)).unwrap();
    raw.push(b' ');
    std::fs::write(blob_file(&tampered, &vsa), raw).unwrap();
    let (_, bytes) = post(
        &mounted.app,
        Some("carol@beta"),
        "beta-repo",
        serde_json::to_vec(&document_of(&tampered)).unwrap(),
    )
    .await;
    let second: Value = serde_json::from_slice(&bytes).unwrap();
    assert_ne!(second["code"], "tampered-blob", "{second}");

    // carol cannot name acme's repository: it is not hers (404).
    let (status, _) = post(
        &mounted.app,
        Some("carol@beta"),
        "mirror",
        serde_json::to_vec(&document).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // alice, in the right account, but another repository than the passport's.
    let outcome = verify(&mounted.app, "alice@acme", "second", &document).await;
    assert_refused(&outcome, "audience-mismatch");
}

#[tokio::test]
async fn a_revoked_or_superseded_decision_is_refused_and_a_down_ledger_is_no_verdict() {
    let world = world().await;
    let mounted = good(&world).await;
    let document = document_of(&world.layout);

    let (status, body) = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    // The ledger cannot be read: 503, no verdict, audited as Failed.
    let broken = mount(
        &world,
        Opts {
            roots: Some(roots_json(&world, |_| {})),
            ledger_down: true,
            ..Opts::default()
        },
    )
    .await;
    let (status, body) = verify(&broken.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_ne!(body["verdict"], "valid");
    assert_eq!(body["verdict"], "none");
    assert_eq!(results(&broken), vec![AuditResult::Failed]);

    // The decision is revoked in the real ledger: refused, immediately.
    world
        .site
        .trust
        .revoke_decision(
            "acme",
            "mirror",
            "dec-1",
            "hrn:hodei:iam:acme:User/carol",
            "cve",
            Utc::now(),
        )
        .await
        .expect("the revocation is written");
    let outcome = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_refused(&outcome, "revoked");

    // Another decision backs the digest now: the passport cites a decision that
    // is no longer the one in force.
    world
        .site
        .trust
        .grant(&[grant(&world.decided, "dec-2")])
        .await
        .expect("a new decision");
    let outcome = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_refused(&outcome, "superseded");
    assert!(results(&mounted).contains(&AuditResult::Denied));
}

#[tokio::test]
async fn the_cost_of_a_document_is_bounded() {
    let world = world().await;
    let mounted = good(&world).await;
    let document = document_of(&world.layout);

    // Past the byte ceiling: 413 before parsing.
    let mut huge = document.clone();
    huge["blobs"].as_array_mut().unwrap().push(json!({
        "digest": format!("sha256:{}", "a".repeat(64)),
        "data_base64": "A".repeat(12 * 1024 * 1024),
    }));
    let (status, _) = post(
        &mounted.app,
        Some("alice@acme"),
        "mirror",
        serde_json::to_vec(&huge).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    // Past the blob count: refused as too large without materializing them.
    let mut many = document.clone();
    for n in 0..600u32 {
        many["blobs"].as_array_mut().unwrap().push(json!({
            "digest": format!("sha256:{n:064x}"),
            "data_base64": "",
        }));
    }
    let outcome = verify(&mounted.app, "alice@acme", "mirror", &many).await;
    assert_refused(&outcome, "too-large");

    // An operator can tighten the ceiling.
    let small = mount(
        &world,
        Opts {
            roots: Some(roots_json(&world, |file| {
                file["max_document_bytes"] = json!(1024)
            })),
            ..Opts::default()
        },
    )
    .await;
    let outcome = verify(&small.app, "alice@acme", "mirror", &document).await;
    assert_eq!(outcome.0, StatusCode::PAYLOAD_TOO_LARGE, "{outcome:?}");
    assert_ne!(outcome.1["verdict"], "valid");
}

// ------------------------------------------------- review hardening (H1..H6)

async fn post_body(app: &Router, repository: &str, body: Body) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/passports/verify?repository={repository}"))
                .header(header::HOST, "127.0.0.1")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, "Bearer alice@acme")
                .body(body)
                .unwrap(),
        )
        .await
        .expect("a response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// A body that sends one byte and then never anything else.
fn trickling_body() -> Body {
    use futures_util::StreamExt;
    Body::from_stream(
        futures_util::stream::once(async {
            Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"{"))
        })
        .chain(futures_util::stream::pending()),
    )
}

#[tokio::test]
async fn a_slow_ledger_is_no_verdict_within_the_deadline_and_the_slot_is_held_then_freed() {
    let world = world().await;
    let document = document_of(&world.layout);
    let mounted = mount(
        &world,
        Opts {
            roots: Some(roots_json(&world, |file| {
                file["ledger_deadline_secs"] = json!(1)
            })),
            ledger: Some(Arc::new(SlowLedger)),
            ..Opts::default()
        },
    )
    .await;

    // One request: the ledger answers in an hour; the answer is 503 within
    // the deadline (a few seconds, never the hour).
    let started = std::time::Instant::now();
    let (status, body) = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
    assert_eq!(body["verdict"], "none");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the ledger phase has a deadline: {:?}",
        started.elapsed()
    );
    assert_eq!(results(&mounted), vec![AuditResult::Failed]);

    // The slot is held through the ledger phase: four verifications waiting
    // on the ledger occupy all four slots, so a fifth is told to retry (429).
    let mut waiting = Vec::new();
    for _ in 0..4 {
        let app = mounted.app.clone();
        let document = document.clone();
        waiting.push(tokio::spawn(async move {
            verify(&app, "alice@acme", "mirror", &document).await
        }));
    }
    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
    let (status, body) = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{body}");
    for task in waiting {
        let (status, _) = task.await.unwrap();
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
    // And freed afterwards: the next one reaches the ledger (503), not 429.
    let (status, body) = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{body}");
}

#[tokio::test]
async fn the_body_is_read_under_a_deadline_before_any_slot_and_its_failures_are_told_apart() {
    let world = world().await;
    let document = document_of(&world.layout);
    let mounted = mount(
        &world,
        Opts {
            roots: Some(roots_json(&world, |file| {
                file["body_deadline_secs"] = json!(1)
            })),
            ..Opts::default()
        },
    )
    .await;

    // Four callers that trickle their body (slowloris) hold no slot: a good
    // request meanwhile is verified, not told 429.
    let mut slow = Vec::new();
    for _ in 0..4 {
        let app = mounted.app.clone();
        slow.push(tokio::spawn(async move {
            post_body(&app, "mirror", trickling_body()).await
        }));
    }
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let (status, body) = verify(&mounted.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for task in slow {
        let (status, body) = task.await.unwrap();
        assert_eq!(status, StatusCode::REQUEST_TIMEOUT, "{body}");
        assert_ne!(body["verdict"], "valid");
    }

    // A body that breaks is a 400, not "too large".
    use futures_util::StreamExt;
    let broken = Body::from_stream(
        futures_util::stream::once(async {
            Ok::<_, std::io::Error>(bytes::Bytes::from_static(b"{"))
        })
        .chain(futures_util::stream::once(async {
            Err(std::io::Error::other("connection reset"))
        })),
    );
    let (status, body) = post_body(&mounted.app, "mirror", broken).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");

    // A body that is too large is still a 413.
    let (status, _) = post_body(
        &mounted.app,
        "mirror",
        Body::from(vec![b' '; 12 * 1024 * 1024]),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    // The audit says exactly what happened.
    let mut seen = results(&mounted);
    seen.sort_by_key(|result| format!("{result:?}"));
    let count = |wanted: AuditResult| seen.iter().filter(|r| **r == wanted).count();
    assert_eq!(count(AuditResult::Allowed), 1, "{seen:?}");
    assert_eq!(
        count(AuditResult::Failed),
        5,
        "four timeouts and a broken body: {seen:?}"
    );
    assert_eq!(
        count(AuditResult::Denied),
        1,
        "the oversized body: {seen:?}"
    );
}

#[tokio::test]
async fn a_file_that_would_trust_more_or_less_than_it_says_does_not_boot() {
    let world = world().await;
    assert!(PassportVerifier::from_json(&roots_json(&world, |_| {})).is_ok());

    // A point of the right length that is not on the curve.
    let off_curve = roots_json(&world, |file| {
        file["provenance_roots"][0]["public_point_hex"] = json!(format!("04{}", "11".repeat(64)));
    });
    let error = PassportVerifier::from_json(&off_curve).unwrap_err();
    assert!(error.to_string().contains("curve"), "{error}");

    // A list of builders of which only the first would be honoured.
    let two_builders = roots_json(&world, |file| {
        file["policy"]["trusted_builders"] =
            json!(["https://builder.example/a", "https://builder.example/b"]);
    });
    assert!(PassportVerifier::from_json(&two_builders).is_err());

    // Deadlines out of range, and fields that have no effect.
    for (field, value) in [
        ("ledger_deadline_secs", 0),
        ("ledger_deadline_secs", 31),
        ("body_deadline_secs", 0),
        ("body_deadline_secs", 61),
    ] {
        let text = roots_json(&world, |file| file[field] = json!(value));
        assert!(
            PassportVerifier::from_json(&text).is_err(),
            "{field}={value}"
        );
    }
    let unknown = roots_json(&world, |file| file["trusted_builder_ids"] = json!(["x"]));
    assert!(PassportVerifier::from_json(&unknown).is_err());

    // A file configured where there is no graph profile is not ignored: the
    // boot refuses the whole configuration.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("roots.json");
    std::fs::write(&path, roots_json(&world, |_| {})).unwrap();
    let mut config = AppConfig::default();
    config.trust.passport_verification_file = Some(path.to_string_lossy().into_owned());
    assert!(config.graph.is_none());
    let booted = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await;
    assert!(booted.is_err());
}

#[tokio::test]
async fn the_operators_minimum_level_is_enforced_on_a_lower_passport() {
    let world = world().await;
    let weak = PassportPolicy {
        builder_identity_proven: false,
        hermetic_declared_complete: false,
        ..hsp160_policy_for_it()
    };
    let layout = export_passport_under(
        &world.bundle_dir,
        "weak.passport",
        &world.alpha,
        &world.alpha,
        &world.signer,
        world.verified_at,
        weak.clone(),
    );
    let document = document_of(&layout);
    let at_least = |minimum: u8| {
        let weak = weak.clone();
        mount(
            &world,
            Opts {
                roots: Some(roots_json(&world, move |file| {
                    file["policy"] = serde_json::to_value(&weak).unwrap();
                    file["min_slsa_level"] = json!(minimum);
                })),
                ..Opts::default()
            },
        )
    };
    let (status, body) = verify(&at_least(1).await.app, "alice@acme", "mirror", &document).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let level: u8 = body["slsa_result"]
        .as_str()
        .unwrap()
        .chars()
        .last()
        .and_then(|digit| digit.to_digit(10))
        .expect("a level") as u8;
    assert!(level < 3, "the weak passport asserts a lower level: {body}");
    for minimum in (level + 1)..=3 {
        assert_refused(
            &verify(
                &at_least(minimum).await.app,
                "alice@acme",
                "mirror",
                &document,
            )
            .await,
            "verification-failed",
        );
    }
}

#[tokio::test]
async fn a_passport_citing_more_decisions_than_a_bundle_can_hold_is_too_large_before_any_crypto() {
    let world = world().await;
    // Room for the document itself: the cap under test is the decisions', not the bytes'.
    let mounted = mount(
        &world,
        Opts {
            roots: Some(roots_json(&world, |file| {
                file["max_document_bytes"] = json!(32 * 1024 * 1024)
            })),
            ..Opts::default()
        },
    )
    .await;
    let big = variant(&world, "big");
    let (position, mut image, _) = find_manifest(&big, PASSPORT_ARTIFACT_TYPE);
    let layers = image["layers"].as_array().unwrap().clone();
    let (at, mut release) = layers
        .iter()
        .enumerate()
        .find_map(|(at, layer)| {
            let bytes = std::fs::read(blob_file(&big, layer["digest"].as_str()?)).ok()?;
            let value: Value = serde_json::from_slice(&bytes).ok()?;
            value["assets"].is_array().then_some((at, value))
        })
        .expect("the release layer");
    let template = release["assets"][0].clone();
    let limit = hodei_artifacts_api::passport_verify::MAX_LEDGER_LOOKUPS;
    release["assets"] = Value::Array(
        (0..=limit)
            .map(|n| {
                let mut asset = template.clone();
                asset["asset_path"] = json!(format!("tools/{n:06}.bin"));
                asset
            })
            .collect(),
    );
    let (digest, size) = put_blob(&big, &serde_json::to_vec(&release).unwrap());
    image["layers"][at]["digest"] = json!(digest);
    image["layers"][at]["size"] = json!(size);
    let (manifest_digest, manifest_size) =
        put_blob(&big, &serde_json::to_vec_pretty(&image).unwrap());
    set_index_entry(&big, position, &manifest_digest, manifest_size);

    let outcome = verify(&mounted.app, "alice@acme", "mirror", &document_of(&big)).await;
    assert_refused(&outcome, "too-large");
    assert_eq!(results(&mounted), vec![AuditResult::Denied]);
}
