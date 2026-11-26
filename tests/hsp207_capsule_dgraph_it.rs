//! HSP-207: reading the release capsule over the graph authority
//! -- what the signed capsule asserts, the graph repeats byte for
//! byte; each read is bounded by the manifest key and a distractor
//! capsule is not reached; revocation is a visible state alongside the
//! history, never a silence that rewrites what was signed.
//!
//! It is gated on HODEI_GRAPH_IT_ENDPOINT like every IT of the stack, and
//! SERIALIZED: the alpha is shared with other workers (HSP-192 GC,
//! HSP-201 OCI), so the assertions are ALWAYS membership checks over the
//! tenant's own keys `hsp207-*` of this run -- never global
//! censuses. The capsule format (manifests, receipts, signature,
//! explicit absence) is HSP-126's offline contract: this IT does not
//! rewrite it, it uses it as the export <-> graph parity oracle.
//!
//! What it tests, against the real engine:
//!
//! - **AC1 parity**: after `ingest`, `read_capsule` returns the exact bytes
//!   of the layout (the image manifest that the signature signs, the
//!   `release.json`, the trust state, the armor), the same release
//!   digest, the same paths with their receipts and their historical
//!   decision, the same absences with their reason -- and the detached signature
//!   still verifies over the bytes the graph stores.
//! - **AC2 boundary**: a distractor capsule (another image digest,
//!   another account and repository, because the alpha is shared) is ingested the same; reading the first
//!   answers exactly its paths, and the distractor's path never
//!   appears in its response. The module's queries are bounded exact
//!   `eq` -- the proof of shape lives in the units.
//! - **Visible revocation AC1+AC2**: once the decision is revoked in the trust
//!   ledger after the export, the graph's capsule keeps the signed
//!   decision intact and `read_capsule_with_trust` reports it `Revoked`;
//!   re-promoting with another decision reports it `Superseded` with the
//!   current decision named. History is not rewritten; today's truth
//!   is read alongside.
//! - **Immutability**: re-ingesting the same bytes is a replay that does not
//!   duplicate; a layout that does not verify does not touch the graph; a key without a
//!   capsule is rejected, not answered empty.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{DateTime, Duration, Utc};
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
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{GraphClient, GraphClientConfig, Tenant};
use hodei_hub::BlobStore;
use hodei_hub::blob::FileBlobStore;
use serde_json::json;

use hodei_artifacts_api::bundles;
use hodei_artifacts_api::bundles::Mapping;
use hodei_artifacts_api::bundles::capsule::LAYOUT_BLOBS;
use hodei_artifacts_api::bundles::capsule::verify_capsule;
use hodei_artifacts_api::bundles::capsule_dgraph::{
    CapsuleGraphError, DecisionState, DgraphCapsuleStore, asset_key, capsule_image_digest,
    capsule_key, withheld_key,
};
use hodei_artifacts_api::bundles::manifest::MANIFEST_NAME;
use hodei_artifacts_api::openpgp::MetadataSigner;
use hodei_artifacts_api::proxy::dgraph_ledger::DgraphProvenanceLedger;

const UPSTREAM_URL: &str = "https://repo.example.org/raw/";

include!("support/hsp219_concurrent_open.rs");

/// A registry with its catalog and its store, in its own directory.
///
/// HSP-314 F4: the catalog and the receipt ledger are the graph's (they used to
/// be the embedded engine, which is ephemeral per test). The alpha is
/// shared and persistent, so each `Site` carries its OWN account and
/// its OWN repository: nothing of one instance touches another's rows, nor
/// those of a previous run. The decision ledger is still the
/// in-memory mirror (`MirrorLedger`): the trust one lives in another slice.
struct Site {
    tmp: tempfile::TempDir,
    store: DgraphCatalogStore,
    ledger: DgraphProvenanceLedger,
    trust: MirrorLedger,
    cas: std::path::PathBuf,
    account: String,
    repository: String,
}

impl Site {
    async fn new(client: Arc<GraphClient>) -> Self {
        static SITE: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock moves")
            .as_nanos();
        let n = SITE.fetch_add(1, Ordering::SeqCst);
        let tmp = tempfile::tempdir().expect("tmp");
        let store = open_store(client.clone()).await;
        let ledger = DgraphProvenanceLedger::open(client)
            .await
            .expect("the receipt ledger opens");
        let cas = tmp.path().join("cas");
        Site {
            tmp,
            store,
            ledger,
            trust: MirrorLedger::new(),
            cas,
            account: format!("it207acme{}{n}", &nanos.to_string()[8..]),
            repository: "mirror".to_string(),
        }
    }

    fn blobs(&self) -> FileBlobStore {
        FileBlobStore::new(&self.cas).expect("the store of content")
    }

    async fn create_proxy(&self, upstream_id: &str) {
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings = json!({
            "proxy": {
                "upstream": { "id": upstream_id, "url": UPSTREAM_URL, "scopes": ["/"] }
            }
        });
        self.store
            .create_repository(
                &self.account,
                RepositoryDescriptor {
                    hrn: repository_hrn(
                        "hodei",
                        "artifacts",
                        "eu",
                        &self.account,
                        &self.repository,
                    ),
                    account: self.account.clone(),
                    repo_key: self.repository.clone(),
                    format: RepositoryFormat::Raw,
                    mode: RepositoryMode::Proxy,
                    config,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
            )
            .await
            .expect("the proxy is created");
    }

    /// What a proxy resolution does: store and publish.
    async fn resolve(&self, path: &str, content: &[u8]) -> ContentDigest {
        let digest = ContentDigest::of_bytes(content);
        self.blobs().put(content).await.expect("the blob is stored");
        self.store
            .publish(
                &self.account,
                Publication {
                    repository: self.repository.clone(),
                    asset_path: path.to_string(),
                    coordinates: PackageCoordinates {
                        package_name: path.to_string(),
                        package_version: digest.value.clone(),
                    },
                    digest: digest.clone(),
                    size_bytes: content.len() as u64,
                    tags: Vec::new(),
                    actor: "hrn:hodei:iam:acme:User/alice".to_string(),
                    correlation_id: format!("corr-{path}"),
                    overwrite: false,
                    precondition: None,
                    subject: None,
                },
            )
            .await
            .expect("the content is published");
        digest
    }

    /// The signed receipt that confirms a resolution.
    async fn receipt(&self, path: &str, digest: &ContentDigest, size_bytes: u64) {
        self.ledger
            .record(&ProvenanceReceipt {
                id: format!(
                    "prv-{}-{}-central-1-fetched",
                    self.account,
                    path.replace('/', "-")
                ),
                account: self.account.clone(),
                repository: self.repository.clone(),
                asset_path: path.to_string(),
                upstream_id: "central".to_string(),
                source_url: format!("{UPSTREAM_URL}{path}"),
                config_version: 1,
                digest: digest.clone(),
                size_bytes,
                checksum: ChecksumEvidence::ComputedOnly,
                status: 200,
                redirects: 0,
                etag: None,
                last_modified: Some((Utc::now() - Duration::hours(1)).to_rfc3339()),
                outcome: ReceiptOutcome::Fetched,
                actor: "hrn:hodei:iam:acme:User/alice".to_string(),
                correlation_id: format!("corr-{path}"),
                fetched_at: Utc::now() - Duration::seconds(2 * 3600),
            })
            .await
            .expect("the receipt is recorded");
    }
}

async fn export_with(
    site: &Site,
    name: &str,
    signer: Option<&MetadataSigner>,
) -> std::path::PathBuf {
    let to = site.tmp.path().join(name);
    let blobs = site.blobs();
    let report = bundles::export(
        &site.store,
        &site.ledger,
        &site.trust,
        &blobs,
        &site.account,
        &site.repository,
        &to,
        signer,
    )
    .await
    .expect("the export runs");
    report.path
}

async fn import_into(
    site: &Site,
    directory: &std::path::Path,
) -> Result<bundles::ImportReport, bundles::BundleError> {
    let blobs = site.blobs();
    bundles::import(
        &site.store,
        &site.store,
        &site.ledger,
        &blobs,
        &site.account,
        &site.repository,
        directory,
        // The destination is another `Site` (another account: the alpha is shared and
        // each site carries its own), so the import crosses owners and
        // declares it explicitly, as HSP-126's contract requires.
        Mapping {
            allow: true,
            overwrite: false,
        },
        "operator",
        "corr-capsule",
    )
    .await
}

const DEC_ID: &str = "dec-h207-1";
const DEC_ID_2: &str = "dec-h207-2";
const ACTOR: &str = "hrn:hodei:iam:acme:User/carol";

fn scratch_signer(tag: &str) -> MetadataSigner {
    MetadataSigner::generate(&format!("HSP-207 {tag} <capsule@example.local>")).expect("a key")
}

fn read_json(path: &Path) -> serde_json::Value {
    let raw = std::fs::read(path).expect("a readable file");
    serde_json::from_slice(&raw).expect("the layout holds JSON")
}

fn blob_hex(digest: &str) -> &str {
    digest.strip_prefix("sha256:").expect("a sha256 digest")
}

/// The digest of the image manifest announced by the index, as the index
/// itself reads it (the order of the HSP-126 oracle: the capsule's
/// manifest is the announced one).
fn index_image_digest(layout: &Path) -> String {
    read_json(&layout.join("index.json"))["manifests"][0]["digest"]
        .as_str()
        .expect("a digest")
        .to_string()
}

/// The exact bytes of the blob the index announces for a digest.
fn layout_blob(layout: &Path, digest: &str) -> Vec<u8> {
    std::fs::read(layout.join(LAYOUT_BLOBS).join(blob_hex(digest))).expect("the layout blob")
}

/// The SHA-256 digest in the layout's form: `sha256:` + lowercase hex.
fn digest_of(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let mut digest = String::from("sha256:");
    for byte in sha2::Sha256::digest(bytes) {
        write!(&mut digest, "{byte:02x}").expect("String");
    }
    digest
}

/// The grants mirror that the trust-aware read consults: the
/// graph's decision ledger lives in another slice; what is tested here
/// is the overlay -- the capsule keeps its history and the ledger
/// answers for the present.
struct MirrorLedger {
    grants: tokio::sync::Mutex<Vec<TrustGrant>>,
}

impl MirrorLedger {
    fn new() -> Self {
        MirrorLedger {
            grants: tokio::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl TrustLedger for MirrorLedger {
    async fn grant(&self, grants: &[TrustGrant]) -> Result<(), CatalogError> {
        self.grants.lock().await.extend_from_slice(grants);
        Ok(())
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
        let mut grants = self.grants.lock().await;
        let mut revoked = 0;
        for grant in grants.iter_mut() {
            if grant.account == account
                && grant.repository == repository
                && grant.decision_id == decision_id
                && grant.revoked_at.is_none()
            {
                grant.revoked_at = Some(at);
                grant.revoked_by = Some(actor.to_string());
                grant.revoke_reason = Some(reason.to_string());
                revoked += 1;
            }
        }
        Ok(revoked)
    }

    async fn revoke_decision_for_account(
        &self,
        account: &str,
        decision_id: &str,
        actor: &str,
        reason: &str,
        at: DateTime<Utc>,
    ) -> Result<u64, CatalogError> {
        let mut grants = self.grants.lock().await;
        let mut revoked = 0;
        for grant in grants.iter_mut() {
            if grant.account == account
                && grant.decision_id == decision_id
                && grant.revoked_at.is_none()
            {
                grant.revoked_at = Some(at);
                grant.revoked_by = Some(actor.to_string());
                grant.revoke_reason = Some(reason.to_string());
                revoked += 1;
            }
        }
        Ok(revoked)
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
        let mut grants = self.grants.lock().await;
        let mut revoked = 0;
        for grant in grants.iter_mut() {
            if grant.account == account
                && grant.repository == repository
                && &grant.digest == digest
                && grant.revoked_at.is_none()
            {
                grant.revoked_at = Some(at);
                grant.revoked_by = Some(actor.to_string());
                grant.revoke_reason = Some(reason.to_string());
                revoked += 1;
            }
        }
        Ok(revoked)
    }

    async fn active_grant(
        &self,
        account: &str,
        repository: &str,
        digest: &ContentDigest,
        at: DateTime<Utc>,
    ) -> Result<Option<TrustGrant>, CatalogError> {
        let grants = self.grants.lock().await;
        Ok(grants
            .iter()
            .filter(|g| {
                g.account == account
                    && g.repository == repository
                    && &g.digest == digest
                    && g.granted_at <= at
                    && g.revoked_at.is_none_or(|revoked| at < revoked)
                    && g.valid_until.is_none_or(|until| at < until)
            })
            .max_by_key(|g| g.granted_at)
            .cloned())
    }

    async fn grants_of(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustGrant>, CatalogError> {
        let grants = self.grants.lock().await;
        Ok(grants
            .iter()
            .filter(|g| g.account == account && g.repository == repository)
            .cloned()
            .collect())
    }
}

fn config_from_env() -> Option<GraphClientConfig> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    let enabled = !ca.is_empty();
    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
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
                tokio::time::sleep(std::time::Duration::from_millis(250 * attempt as u64)).await;
                continue;
            }
        };
        match client.login().await {
            Ok(()) => return Some(Arc::new(client)),
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(250 * attempt as u64)).await;
            }
        }
    }
    panic!("Dgraph did not open after 3 attempts: {last}");
}

/// This run's tenant: `hsp207-` + nanos + counter, as in
/// the neighboring ITs. Nothing in one test touches another's key space.
fn run_tenant(tag: &str) -> Tenant {
    static RUN: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock moves")
        .as_nanos() as u64;
    let name = format!(
        "hsp207-{tag}-{}-{}",
        nanos,
        RUN.fetch_add(1, Ordering::SeqCst)
    );
    Tenant::new(&name).expect("the run tenant is valid")
}

/// The evidence directory: `target/uat/hsp207/`.
fn the_evidence_dir() -> PathBuf {
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp207");
    std::fs::create_dir_all(&dir).expect("the evidence directory");
    dir
}

fn record_evidence(test: &str, line: &str) {
    let file = the_evidence_dir().join("evidencia.md");
    let entry = format!("- [{test}] {line}\n");
    use std::io::Write as _;
    if let Ok(mut out) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)
    {
        let _ = writeln!(out, "{}", entry.trim_end());
    }
}

/// How many nodes there are under an exact graph key, for any
/// predicate of this slice: the check is a MEMBERSHIP check of the
/// own keys -- never a global census over the shared alpha.
async fn copies_of(client: &GraphClient, predicate: &str, key: &str) -> usize {
    let query = format!(
        "{{ q(func: eq({predicate}, {key})) {{ uid }} }}",
        key = dql_string(key)
    );
    let response = client
        .execute_dql(&query)
        .await
        .expect("the membership query runs");
    response
        .json
        .get("q")
        .and_then(serde_json::Value::as_array)
        .map(|rows| rows.len())
        .expect("the answer has the block")
}

/// The central scenario's package: a path with a decision (`tools/a.bin`),
/// one confirmed without a decision (`tools/b.bin`) and one withheld (`tools/c.bin`)
/// -- explicit absence is part of the contract.
async fn capsule_site(
    client: Arc<GraphClient>,
    decided: &mut ContentDigest,
) -> (Site, PathBuf, MetadataSigner) {
    let site = Site::new(client).await;
    site.create_proxy("central").await;
    let a = site.resolve("tools/a.bin", b"tool a").await;
    site.receipt("tools/a.bin", &a, 6).await;
    *decided = a.clone();
    let b = site.resolve("tools/b.bin", b"tool b").await;
    site.receipt("tools/b.bin", &b, 6).await;
    // c.bin has no receipt: it is withheld and the capsule says so.
    site.resolve("tools/c.bin", b"tool c").await;

    let signer = scratch_signer("capsule");
    site.trust
        .grant(&[TrustGrant {
            account: site.account.clone(),
            repository: site.repository.clone(),
            digest: a,
            decision_id: DEC_ID.to_string(),
            actor: ACTOR.to_string(),
            granted_at: Utc::now() - Duration::days(1),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: None,
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: None,
            risk_version: None,
            valid_until: Some(Utc::now() + Duration::hours(1)),
        }])
        .await
        .expect("the grant exists");

    let directory = export_with(&site, "bundle", Some(&signer)).await;
    (site, directory, signer)
}

/// Grants the digest with the named decision, for the validity
/// overlay that `read_capsule_with_trust` reads.
async fn seed_grant(ledger: &MirrorLedger, site: &Site, digest: &ContentDigest, decision_id: &str) {
    ledger
        .grant(&[TrustGrant {
            account: site.account.clone(),
            repository: site.repository.clone(),
            digest: digest.clone(),
            decision_id: decision_id.to_string(),
            actor: ACTOR.to_string(),
            granted_at: Utc::now() - Duration::days(2),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: None,
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: None,
            risk_version: None,
            valid_until: None,
        }])
        .await
        .expect("the mirror grant lands");
}

/// AC1: reading from the graph is the layout, byte for byte, with the
/// signature alive. AC2: bounded by the manifest key, with the distractor
/// present and unreachable from the other capsule.
#[tokio::test]
async fn the_graph_reads_the_capsule_the_layout_signed() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let tenant = run_tenant("parity");
    let mut decided = ContentDigest::sha256("0".repeat(64));
    let (_site, directory, signer) = capsule_site(client.clone(), &mut decided).await;
    let layout = directory.join("oci");
    let verified = verify_capsule(&layout, &signer).expect("the layout verifies");

    let store = DgraphCapsuleStore::open(client.clone(), tenant.clone())
        .await
        .expect("the capsule store opens on the validated schema");

    // The digest the module names is the one the index announces and the
    // one the signature signs: the same key at both doors.
    let image_digest = index_image_digest(&layout);
    assert_eq!(
        image_digest,
        capsule_image_digest(&layout).expect("the slice names the same digest"),
        "the capsule image is the digest both readers announce"
    );
    let ingest = store
        .ingest(&layout, &signer)
        .await
        .expect("the verified layout is ingested");
    assert_eq!(ingest.image_digest, image_digest);
    assert!(!ingest.replayed, "a first ingest writes");

    // Own membership: THIS capsule once, THIS path once.
    assert_eq!(
        copies_of(
            &client,
            "hodei.capsule.key",
            &capsule_key(&tenant, &image_digest)
        )
        .await,
        1,
        "the capsule row is one"
    );
    assert_eq!(
        copies_of(
            &client,
            "hodei.capsule.asset.key",
            &asset_key(&tenant, &image_digest, "tools/a.bin")
        )
        .await,
        1,
        "the decided path row is one"
    );

    // AC1 -- byte-for-byte parity with the offline layout.
    let graph = store
        .read_capsule(&image_digest)
        .await
        .expect("the capsule reads back from the graph");
    assert_eq!(
        graph.release_bytes,
        std::fs::read(directory.join(MANIFEST_NAME)).expect("the bundle is on disk"),
        "the bundle bytes are the bytes the layout wrote"
    );
    assert_eq!(
        graph.manifest, verified.manifest,
        "the bundle says the same"
    );
    assert_eq!(
        graph.image_manifest_bytes,
        layout_blob(&layout, &image_digest)
    );
    assert_eq!(
        graph.release_digest,
        digest_of(&graph.release_bytes),
        "the announced release digest is the digest of the stored bytes"
    );
    assert_eq!(
        graph.capsule, verified.capsule,
        "the trust state is the exported one"
    );
    assert_eq!(graph.exported_at, verified.capsule.exported_at);
    assert_eq!(graph.signer, verified.signer);
    // The detached signature verifies over the bytes the graph stores:
    // the graph's capsule is the signed capsule, not a translation.
    graph
        .verify_signature(&signer)
        .expect("the signature holds over the graph's bytes");

    // The history and the explicit absence, intact.
    let a = graph
        .assets
        .iter()
        .find(|row| row.asset_path == "tools/a.bin")
        .expect("the decided path answers");
    let decision = a
        .decision
        .as_ref()
        .expect("the capsule carries its decision");
    assert_eq!(decision.decision_id, DEC_ID);
    assert_eq!(decision.actor, ACTOR);
    assert_eq!(a.receipt_id, verified.manifest.assets[0].receipt.id);
    assert_eq!(a.digest, verified.manifest.assets[0].digest);
    assert_eq!(a.size_bytes, 6);
    let b = graph
        .assets
        .iter()
        .find(|row| row.asset_path == "tools/b.bin")
        .expect("the bare path answers");
    assert!(b.decision.is_none(), "a null decision stays null");
    assert_eq!(graph.assets.len(), 2, "only the two resolved paths answer");
    assert_eq!(graph.withheld.len(), 1);
    assert_eq!(graph.withheld[0].asset_path, "tools/c.bin");
    assert!(
        !graph.withheld[0].reason.is_empty(),
        "the absence explains itself"
    );

    // AC2 -- the distractor: another signed capsule, same tenant and same
    // repository. It is ingested under its own key; reading the first
    // never reaches its path.
    let other_site = Site::new(client.clone()).await;
    other_site.create_proxy("central").await;
    let d = other_site.resolve("tools/z.bin", b"tool z").await;
    other_site.receipt("tools/z.bin", &d, 6).await;
    let other_directory = export_with(&other_site, "other", Some(&signer)).await;
    let other_layout = other_directory.join("oci");
    verify_capsule(&other_layout, &signer).expect("the distractor verifies");
    let other_digest = index_image_digest(&other_layout);
    assert_ne!(other_digest, image_digest, "a different capsule image");
    let other_ingest = store
        .ingest(&other_layout, &signer)
        .await
        .expect("the distractor is ingested under its own key");
    assert!(!other_ingest.replayed);

    let first = store
        .read_capsule(&image_digest)
        .await
        .expect("the first capsule still reads");
    let paths: Vec<&str> = first.assets.iter().map(|r| r.asset_path.as_str()).collect();
    assert!(
        !paths.contains(&"tools/z.bin"),
        "the distractor's path never joins this capsule: {paths:?}"
    );
    assert_eq!(
        first.assets.len(),
        2,
        "the read is bounded by the manifest key"
    );
    let withheld_paths: Vec<&str> = first
        .withheld
        .iter()
        .map(|w| w.asset_path.as_str())
        .collect();
    assert!(!withheld_paths.contains(&"tools/z.bin"));

    // The distractor's key is alive and IS its own: the boundary is one of
    // membership -- each capsule answers for its key, nothing is merged.
    let second = store
        .read_capsule(&other_digest)
        .await
        .expect("the distractor reads by its own key");
    assert_eq!(second.assets.len(), 1);
    assert_eq!(second.assets[0].asset_path, "tools/z.bin");
    assert!(second.withheld.is_empty());
    assert_ne!(second.image_digest, first.image_digest);

    // HSP-126's offline contract is still alive with the graph in front:
    // the SAME exported package is imported intact into another destination.
    // Reading the capsule in the graph does not change how an already
    // exported package looks -- export <-> graph parity is additive.
    let target = Site::new(client.clone()).await;
    target.create_proxy("central").await;
    let report = import_into(&target, &directory)
        .await
        .expect("the destination accepts the package");
    assert_eq!(
        report.imported, 2,
        "las mismas dos rutas que la capsula del grafo declara"
    );

    record_evidence(
        "the_graph_reads_the_capsule_the_layout_signed",
        &format!(
            "paridad byte a byte + firma viva sobre digest {image_digest}; \
             distractor {other_digest} aislado por clave; membrecia 1+1; sin censos; \
             import offline HSP-126 intacto (2 rutas)"
        ),
    );
}

/// AC1 + AC2 -- revocation does not hide and does not rewrite: the capsule
/// keeps its signed decision; the ledger says where it stands today.
#[tokio::test]
async fn a_revocation_is_visible_beside_the_record_it_never_rewrites_it() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let tenant = run_tenant("revoke");
    let mut decided = ContentDigest::sha256("0".repeat(64));
    let (site, directory, signer) = capsule_site(client.clone(), &mut decided).await;
    let layout = directory.join("oci");
    verify_capsule(&layout, &signer).expect("the layout verifies");

    let store = DgraphCapsuleStore::open(client.clone(), tenant.clone())
        .await
        .expect("the store opens");
    let image_digest = index_image_digest(&layout);
    store
        .ingest(&layout, &signer)
        .await
        .expect("the capsule is ingested");

    let ledger = MirrorLedger::new();
    seed_grant(&ledger, &site, &decided, DEC_ID).await;
    let now = Utc::now();

    // Today the current decision IS the capsule's: Active.
    let live = store
        .read_capsule_with_trust(&image_digest, &ledger, now)
        .await
        .expect("the capsule reads with trust");
    assert_eq!(
        live.decision_state_of("tools/a.bin"),
        Some(&DecisionState::Active),
        "the grant backs the digest"
    );

    // It is revoked after the export. The capsule is NOT rewritten: the
    // release's byte is still the signed byte.
    ledger
        .revoke_decision(
            &site.account,
            &site.repository,
            DEC_ID,
            "hrn:hodei:iam:acme:User/dave",
            "the risk changed",
            now,
        )
        .await
        .expect("the revocation lands");
    let after = store
        .read_capsule_with_trust(&image_digest, &ledger, now + Duration::minutes(1))
        .await
        .expect("the capsule reads after the revocation");
    assert_eq!(
        after.decision_state_of("tools/a.bin"),
        Some(&DecisionState::Revoked),
        "the revocation is never hidden"
    );
    // The history intact: the decision is still named, with its actor.
    let a = after
        .assets
        .iter()
        .find(|row| row.asset_path == "tools/a.bin")
        .expect("the row answers");
    let decision = a.decision.as_ref().expect("the decision is still there");
    assert_eq!(
        decision.decision_id, DEC_ID,
        "a signed capsule is not rewritten"
    );
    assert_eq!(decision.actor, ACTOR);
    assert_eq!(
        after.release_bytes, live.release_bytes,
        "the exact bytes the signature covers did not move"
    );
    after
        .verify_signature(&signer)
        .expect("the signature still holds");
    // Without a historical decision, no state to revoke: NoDecision.
    assert_eq!(
        after.decision_state_of("tools/b.bin"),
        Some(&DecisionState::NoDecision)
    );

    // Re-promotion with ANOTHER decision: the capsule keeps its own and the
    // state names the current one -- Superseded, never the old decision.
    seed_grant(&ledger, &site, &decided, DEC_ID_2).await;
    let superseded = store
        .read_capsule_with_trust(&image_digest, &ledger, now + Duration::hours(1))
        .await
        .expect("the capsule reads after the re-promotion");
    assert_eq!(
        superseded.decision_state_of("tools/a.bin"),
        Some(&DecisionState::Superseded {
            current: DEC_ID_2.to_string()
        }),
        "the active grant is another decision"
    );
    let a = superseded
        .assets
        .iter()
        .find(|row| row.asset_path == "tools/a.bin")
        .expect("the row");
    assert_eq!(
        a.decision.as_ref().expect("history").decision_id,
        DEC_ID,
        "the recorded decision is the exported one"
    );

    record_evidence(
        "a_revocation_is_visible_beside_the_record_it_never_rewrites_it",
        &format!(
            "digest {image_digest}: Active -> Revoked (bytes y decision intactos) -> \
             Superseded({DEC_ID_2}); b.bin siempre NoDecision; firma viva"
        ),
    );
}

/// Registry immutability: the replay does not duplicate; a layout that does not
/// verify does not touch the graph; a key without a capsule is rejected.
#[tokio::test]
async fn a_signed_capsule_is_never_rewritten_in_the_graph() {
    let Some(client) = fresh_client().await else {
        return;
    };
    let tenant = run_tenant("immutable");
    let mut decided = ContentDigest::sha256("0".repeat(64));
    let (_site, directory, signer) = capsule_site(client.clone(), &mut decided).await;
    let layout = directory.join("oci");
    let image_digest = index_image_digest(&layout);

    let store = DgraphCapsuleStore::open(client.clone(), tenant.clone())
        .await
        .expect("the store opens");
    store
        .ingest(&layout, &signer)
        .await
        .expect("the first ingest writes");

    // The same layout again: replay. Neither the capsule nor its rows
    // duplicate (exact membership by own key, never a census).
    let replay = store
        .ingest(&layout, &signer)
        .await
        .expect("the same bytes replay");
    assert!(replay.replayed, "the graph already had these bytes");
    assert_eq!(
        copies_of(
            &client,
            "hodei.capsule.key",
            &capsule_key(&tenant, &image_digest)
        )
        .await,
        1
    );
    for path in ["tools/a.bin", "tools/b.bin"] {
        assert_eq!(
            copies_of(
                &client,
                "hodei.capsule.asset.key",
                &asset_key(&tenant, &image_digest, path)
            )
            .await,
            1,
            "the row for {path} is one"
        );
    }
    assert_eq!(
        copies_of(
            &client,
            "hodei.capsule.withheld.key",
            &withheld_key(&tenant, &image_digest, "tools/c.bin")
        )
        .await,
        1,
        "the absence row is one"
    );

    // A layout whose image capsule is not what the index announces
    // does not enter: the ingest's offline gate catches it BEFORE writing and
    // the graph stays intact.
    let forged = directory.join("oci-forged");
    copy_layout_dir(&layout, &forged).expect("the layout is copied");
    let capsule_blob = forged.join(LAYOUT_BLOBS).join(blob_hex(&image_digest));
    let poisoned = {
        let mut image: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&capsule_blob).expect("the image"))
                .expect("JSON");
        image["annotations"] = serde_json::json!({"hodei.test": "poisoned"});
        serde_json::to_vec(&image).expect("the poisoned image")
    };
    std::fs::write(&capsule_blob, &poisoned).expect("the poisoned blob is written");
    let error = store
        .ingest(&forged, &signer)
        .await
        .expect_err("a poisoned capsule does not verify");
    assert!(
        matches!(error, CapsuleGraphError::Unsound(_)),
        "the offline gate refuses the bytes: {error}"
    );
    assert_eq!(
        copies_of(
            &client,
            "hodei.capsule.key",
            &capsule_key(&tenant, &image_digest)
        )
        .await,
        1,
        "nothing was written by the refused ingest"
    );
    let graph = store
        .read_capsule(&image_digest)
        .await
        .expect("the untouched capsule still reads exactly");
    graph
        .verify_signature(&signer)
        .expect("the stored signature still holds");

    // And a nonexistent key is answered as Refused, not as empty:
    // reading something that was not ingested is an error, never a mute capsule.
    let error = store
        .read_capsule(&format!("sha256:{}", "f".repeat(64)))
        .await
        .expect_err("no capsule under this name");
    assert!(
        matches!(error, CapsuleGraphError::Refused(_)),
        "a missing capsule is refused: {error}"
    );

    record_evidence(
        "a_signed_capsule_is_never_rewritten_in_the_graph",
        &format!(
            "digest {image_digest}: replay sin duplicar (1+1+1+1), layout envenenado \
             rechazado Unsound ANTES de escribir, clave ajena Refused"
        ),
    );
}

fn copy_layout_dir(from: &Path, to: &Path) -> Result<(), std::io::Error> {
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_layout_dir(&path, &target)?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// The capsule's manifest is the one announced by the index; the signature's
/// does not enter the key: this test states that for the reader.
#[test]
fn a_layout_that_is_not_there_is_refused_not_invented() {
    // The graph's entry door is offline: a layout that does not exist
    // does not produce an empty capsule, it produces Unsound. Nothing the
    // graph stores can rest on a layout that was not verified.
    let error = capsule_image_digest(Path::new("/this/path/does/not/exist"))
        .expect_err("a missing layout is an error");
    assert!(
        matches!(error, CapsuleGraphError::Unsound(_)),
        "missing layout: {error}"
    );
}
