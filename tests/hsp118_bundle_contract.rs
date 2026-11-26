//! The resolution bundle contract: export what was resolved, import
//! without network and make nothing visible that is not backed.
//!
//! What is checked here is the value of HAR-24: a site with no outbound
//! network access can resolve against a proxy by importing the bundle, the origin
//! is provenance and not authority, what is missing is explainable, a
//! rotten bundle publishes nothing, and crossing owners requires saying so explicitly.

use chrono::{Duration, Utc};
use hodei_catalog::model::{
    DigestScope, PackageCoordinates, Publication, RepositoryConfig, RepositoryDescriptor,
    RepositoryFormat, RepositoryMode, repository_hrn,
};
use hodei_catalog::{
    CatalogReader, CatalogWriter, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt,
    ReceiptOutcome, TrustGrant, TrustLedger,
};
use hodei_catalog_storage::DgraphCatalogStore;
use hodei_evidence::ContentDigest;
use hodei_hub::BlobStore;
use hodei_hub::blob::FileBlobStore;
use serde_json::json;

use hodei_artifacts_api::bundles::{self, BundleManifest, Mapping, manifest::FORMAT};

// The graph profile of the harness: a REAL Dgraph namespace per site.
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

const UPSTREAM_URL: &str = "https://repo.example.org/raw/";

/// A registry with its catalog and its store, in a directory of its own.
struct Site {
    tmp: Workdir,
    store: DgraphCatalogStore,
    trust: hodei_trust_policy_storage::DgraphTrustStore,
    provenance: hodei_artifacts_api::proxy::dgraph_ledger::DgraphProvenanceLedger,
    cas: std::path::PathBuf,
}

impl Site {
    async fn new() -> Self {
        // A REAL Dgraph namespace per site, with the real catalog store.
        let tmp = Workdir::new();
        let store = tmp.tenant().catalog_store().await;
        let trust = tmp.tenant().trust_store().await;
        let provenance = tmp.tenant().provenance_ledger().await;
        let cas = tmp.path().join("cas");
        Site {
            tmp,
            store,
            trust,
            provenance,
            cas,
        }
    }

    fn blobs(&self) -> FileBlobStore {
        FileBlobStore::new(&self.cas).expect("the store of content")
    }

    async fn create_proxy(&self, account: &str, repo_key: &str, upstream_id: &str) {
        let mut config = RepositoryConfig::new(DigestScope::Global);
        config.config_settings = json!({
            "proxy": {
                "upstream": { "id": upstream_id, "url": UPSTREAM_URL, "scopes": ["/"] }
            }
        });
        self.store
            .create_repository(
                account,
                RepositoryDescriptor {
                    hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
                    account: account.to_string(),
                    repo_key: repo_key.to_string(),
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

    async fn create_hosted(&self, account: &str, repo_key: &str) {
        let config = RepositoryConfig::new(DigestScope::Global);
        self.store
            .create_repository(
                account,
                RepositoryDescriptor {
                    hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
                    account: account.to_string(),
                    repo_key: repo_key.to_string(),
                    format: RepositoryFormat::Raw,
                    mode: RepositoryMode::Hosted,
                    config,
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
            )
            .await
            .expect("the hosted repository is created");
    }

    /// What a proxy resolution does: store, publish and sign the
    /// receipt that confirms it.
    async fn resolve(
        &self,
        account: &str,
        repository: &str,
        path: &str,
        content: &[u8],
    ) -> ContentDigest {
        let digest = ContentDigest::of_bytes(content);
        self.blobs().put(content).await.expect("the blob is stored");
        self.store
            .publish(
                account,
                Publication {
                    repository: repository.to_string(),
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

    #[allow(clippy::too_many_arguments)]
    async fn receipt(
        &self,
        account: &str,
        repository: &str,
        path: &str,
        digest: &ContentDigest,
        size_bytes: u64,
        outcome: ReceiptOutcome,
        upstream_id: &str,
        config_version: u64,
    ) {
        self.receipt_at(
            account,
            repository,
            path,
            digest,
            size_bytes,
            outcome,
            upstream_id,
            config_version,
            2 * 3600,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn receipt_at(
        &self,
        account: &str,
        repository: &str,
        path: &str,
        digest: &ContentDigest,
        size_bytes: u64,
        outcome: ReceiptOutcome,
        upstream_id: &str,
        config_version: u64,
        age_secs: i64,
    ) {
        self.provenance
            .record(&ProvenanceReceipt {
                // The id is a graph key: the production ids are UUIDs, so the
                // slashes of the path cannot ride in it.
                id: format!(
                    "prv-{}-{upstream_id}-{config_version}-{}",
                    path.replace('/', "_"),
                    outcome.as_str()
                ),
                account: account.to_string(),
                repository: repository.to_string(),
                asset_path: path.to_string(),
                upstream_id: upstream_id.to_string(),
                source_url: format!("{UPSTREAM_URL}{path}"),
                config_version,
                digest: digest.clone(),
                size_bytes,
                checksum: ChecksumEvidence::ComputedOnly,
                status: 200,
                redirects: 0,
                etag: None,
                last_modified: Some((Utc::now() - Duration::hours(1)).to_rfc3339()),
                outcome,
                actor: "hrn:hodei:iam:acme:User/alice".to_string(),
                correlation_id: format!("corr-{path}"),
                fetched_at: Utc::now() - Duration::seconds(age_secs),
            })
            .await
            .expect("the receipt is recorded");
    }
}

/// Exports the origin into the `name` directory of its own temp dir.
async fn export(site: &Site, name: &str, account: &str, repository: &str) -> std::path::PathBuf {
    let to = site.tmp.path().join(name);
    let blobs = site.blobs();
    let report = bundles::export(
        &site.store,
        &site.provenance,
        &site.trust,
        &blobs,
        account,
        repository,
        &to,
        None,
    )
    .await
    .expect("the export runs");
    report.path
}

async fn import(
    site: &Site,
    directory: &std::path::Path,
    account: &str,
    repository: &str,
    mapping: Mapping,
) -> Result<bundles::ImportReport, bundles::BundleError> {
    let blobs = site.blobs();
    bundles::import(
        &site.store,
        &site.store,
        &site.provenance,
        &blobs,
        account,
        repository,
        directory,
        mapping,
        "operator",
        "corr-import",
    )
    .await
}

fn read_manifest(directory: &std::path::Path) -> BundleManifest {
    let raw = std::fs::read(directory.join("bundle.json")).expect("the manifest is there");
    serde_json::from_slice(&raw).expect("the manifest is read")
}

#[tokio::test]
async fn the_bundle_carries_resolved_content_and_leaves_the_rest_explained() {
    let site = Site::new().await;
    site.create_proxy("acme", "mirror", "central").await;

    // Resolved and confirmed: it gets in.
    let good = site
        .resolve("acme", "mirror", "tools/a.bin", b"tool a")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/a.bin",
        &good,
        6,
        ReceiptOutcome::Fetched,
        "central",
        1,
    )
    .await;

    // Held back: there was a confirmation, but the most recent receipt is a
    // quarantine in force.
    let quarantined = site
        .resolve("acme", "mirror", "tools/b.bin", b"tool b")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/b.bin",
        &quarantined,
        6,
        ReceiptOutcome::Fetched,
        "central",
        1,
    )
    .await;
    let offered = ContentDigest::of_bytes(b"tool b broken");
    site.receipt_at(
        "acme",
        "mirror",
        "tools/b.bin",
        &offered,
        13,
        ReceiptOutcome::Quarantined,
        "central",
        1,
        60,
    )
    .await;

    // Confirmed by a configuration that is no longer in force: it is held back.
    let stale = site
        .resolve("acme", "mirror", "tools/c.bin", b"tool c")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/c.bin",
        &stale,
        6,
        ReceiptOutcome::Fetched,
        "central",
        5,
    )
    .await;

    // Confirmed but without bytes in the store: it is held back, nothing is invented.
    let missing = site
        .resolve("acme", "mirror", "tools/d.bin", b"tool d")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/d.bin",
        &missing,
        6,
        ReceiptOutcome::Revalidated,
        "central",
        1,
    )
    .await;
    std::fs::remove_file(
        FileBlobStore::new(&site.cas)
            .expect("store")
            .the_path_of(&missing),
    )
    .expect("the blob is taken away");

    // Promoted by a decision: the decision comes out as evidence.
    let promoted = site
        .resolve("acme", "mirror", "tools/e.bin", b"tool e")
        .await;
    site.receipt(
        "acme",
        "mirror",
        "tools/e.bin",
        &promoted,
        6,
        ReceiptOutcome::Fetched,
        "central",
        1,
    )
    .await;
    site.trust
        .grant(&[TrustGrant {
            account: "acme".to_string(),
            repository: "mirror".to_string(),
            digest: promoted.clone(),
            decision_id: "dec-1".to_string(),
            actor: "hrn:hodei:iam:acme:User/carol".to_string(),
            granted_at: Utc::now() - Duration::days(1),
            revoked_at: None,
            revoked_by: None,
            revoke_reason: None,
            subject: None,
            policy_version: Some("1.0.0".to_string()),
            capsule_hash: None,
            risk_version: None,
            valid_until: Some(Utc::now() + chrono::Duration::hours(1)),
        }])
        .await
        .expect("the grant is written");

    let directory = export(&site, "out", "acme", "mirror").await;
    let manifest = read_manifest(&directory);

    let paths: Vec<&str> = manifest
        .assets
        .iter()
        .map(|e| e.asset_path.as_str())
        .collect();
    assert_eq!(paths, ["tools/a.bin", "tools/e.bin"]);
    assert_eq!(manifest.format, FORMAT);
    assert_eq!(manifest.origin.upstream_id, "central");
    assert_eq!(manifest.origin.config_version, 1);
    assert_eq!(manifest.assets[0].package, "tools/a.bin");
    assert_eq!(manifest.assets[0].version, good.value);
    assert_eq!(manifest.assets[0].size_bytes, 6);
    assert!(manifest.assets[0].decision.is_none());
    let decision = manifest.assets[1]
        .decision
        .as_ref()
        .expect("the decision travels");
    assert_eq!(decision.decision_id, "dec-1");
    assert_eq!(decision.actor, "hrn:hodei:iam:acme:User/carol");

    // The quarantine in force overrides its earlier confirmation, and the
    // absence explains itself.
    assert_eq!(manifest.withheld.len(), 3);
    for (path, prefix) in [
        ("tools/b.bin", "the origin has this content quarantined"),
        (
            "tools/c.bin",
            "the confirming receipt speaks of an older configuration",
        ),
        (
            "tools/d.bin",
            "the content is not in the store of the origin",
        ),
    ] {
        let withheld = manifest
            .withheld
            .iter()
            .find(|w| w.asset_path == path)
            .unwrap_or_else(|| panic!("{path} is withheld"));
        assert!(
            withheld.reason.starts_with(prefix),
            "{path}: {}",
            withheld.reason
        );
    }
    let paths: Vec<&str> = manifest
        .withheld
        .iter()
        .map(|w| w.asset_path.as_str())
        .collect();
    assert_eq!(paths, ["tools/b.bin", "tools/c.bin", "tools/d.bin"]);

    // The files are in the store layout, and the bundle does not carry
    // the upstream URL as configuration: only as per-file
    // evidence, without secrets.
    assert!(
        directory
            .join(BundleManifest::blob_file(&manifest.assets[0].digest))
            .is_file()
    );
    let text = std::fs::read_to_string(directory.join("bundle.json")).expect("read");
    assert!(
        !text.contains("\"url\""),
        "the package carries no upstream configuration"
    );
    assert!(!text.contains("x-checksum"), "no secret headers travel");
}

#[tokio::test]
async fn an_imported_package_resolves_without_the_network() {
    let origin = Site::new().await;
    origin.create_proxy("acme", "mirror", "central").await;
    let digest = origin
        .resolve("acme", "mirror", "tools/a.bin", b"tool a")
        .await;
    origin
        .receipt(
            "acme",
            "mirror",
            "tools/a.bin",
            &digest,
            6,
            ReceiptOutcome::Fetched,
            "central",
            1,
        )
        .await;
    let directory = export(&origin, "out", "acme", "mirror").await;

    // The destination is a new site with its own configuration: same
    // upstream authority because the operator configured it the same, and the
    // receipt written here is signed by THAT configuration.
    let destination = Site::new().await;
    destination.create_proxy("acme", "mirror", "central").await;
    let report = import(
        &destination,
        &directory,
        "acme",
        "mirror",
        Mapping::default(),
    )
    .await
    .expect("the import runs");
    assert_eq!(report.imported, 1);
    assert_eq!(report.already_present, 0);
    assert_eq!(report.blob_bytes, 6);

    let asset = destination
        .store
        .asset("acme", "mirror", "tools/a.bin")
        .await
        .expect("the path is published");
    assert_eq!(asset.digest, digest);

    let receipts = destination
        .provenance
        .receipts_of("acme", "mirror", "tools/a.bin")
        .await
        .expect("the ledger answers");
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].outcome, ReceiptOutcome::Imported);
    assert_eq!(receipts[0].upstream_id, "central");
    assert_eq!(
        receipts[0].config_version, 1,
        "the destination signs its own receipt"
    );

    // The bytes arrived and verify against their digest.
    let content = destination
        .blobs()
        .get(&digest)
        .await
        .expect("the blob verifies in the destination store");
    assert_eq!(content, b"tool a");

    // Repeating the import neither duplicates nor rewrites: the receipt is signed
    // with the same identity and the path already points to the same content.
    let again = import(
        &destination,
        &directory,
        "acme",
        "mirror",
        Mapping::default(),
    )
    .await
    .expect("the import is idempotent");
    assert_eq!(again.imported, 0);
    assert_eq!(again.already_present, 1);
    assert_eq!(
        destination
            .provenance
            .receipts_of("acme", "mirror", "tools/a.bin")
            .await
            .expect("the ledger answers")
            .len(),
        1
    );
}

#[tokio::test]
async fn a_corrupt_package_makes_nothing_visible() {
    let origin = Site::new().await;
    origin.create_proxy("acme", "mirror", "central").await;
    let good = origin
        .resolve("acme", "mirror", "tools/a.bin", b"tool a")
        .await;
    origin
        .receipt(
            "acme",
            "mirror",
            "tools/a.bin",
            &good,
            6,
            ReceiptOutcome::Fetched,
            "central",
            1,
        )
        .await;
    let other = origin
        .resolve("acme", "mirror", "tools/z.bin", b"tool z")
        .await;
    origin
        .receipt(
            "acme",
            "mirror",
            "tools/z.bin",
            &other,
            6,
            ReceiptOutcome::Revalidated,
            "central",
            1,
        )
        .await;
    let directory = export(&origin, "out", "acme", "mirror").await;

    // A rotten byte in a blob: the manifest denies it and nothing is published.
    let blob = directory.join(BundleManifest::blob_file(&other.to_string()));
    std::fs::write(&blob, b"tool x tampered").expect("the file is poisoned");

    let destination = Site::new().await;
    destination.create_proxy("acme", "mirror", "central").await;
    let error = import(
        &destination,
        &directory,
        "acme",
        "mirror",
        Mapping::default(),
    )
    .await
    .expect_err("the package is refused");
    assert!(
        matches!(&error, bundles::BundleError::Unsound(_)),
        "the error is about the package: {error}"
    );
    assert!(
        destination
            .store
            .asset("acme", "mirror", "tools/a.bin")
            .await
            .is_err(),
        "verify-before-visible: ni la ruta sana del paquete es visible"
    );
    assert!(
        destination
            .provenance
            .receipts_of("acme", "mirror", "tools/a.bin")
            .await
            .expect("the ledger answers")
            .is_empty()
    );
}

#[tokio::test]
async fn provenance_is_not_authority() {
    let origin = Site::new().await;
    origin.create_proxy("acme", "mirror", "central").await;
    let digest = origin
        .resolve("acme", "mirror", "tools/a.bin", b"tool a")
        .await;
    origin
        .receipt(
            "acme",
            "mirror",
            "tools/a.bin",
            &digest,
            6,
            ReceiptOutcome::Fetched,
            "central",
            1,
        )
        .await;
    let directory = export(&origin, "out", "acme", "mirror").await;

    // A destination that resolves against another upstream cannot receive the
    // bundle: the provenance of the origin does not authorize here.
    let wrong_upstream = Site::new().await;
    wrong_upstream.create_proxy("acme", "mirror", "nexus").await;
    let error = import(
        &wrong_upstream,
        &directory,
        "acme",
        "mirror",
        Mapping::default(),
    )
    .await
    .expect_err("the upstreams disagree");
    assert!(error.to_string().contains("nexus"), "{error}");
    assert!(error.to_string().contains("central"), "{error}");

    // And a destination that is not a proxy does not receive it either.
    let hosted = Site::new().await;
    hosted.create_hosted("acme", "vault").await;
    let error = import(&hosted, &directory, "acme", "vault", Mapping::default())
        .await
        .expect_err("a hosted repository does not resolve");
    assert!(error.to_string().contains("hosted"), "{error}");

    // Without a destination repository, the missing step is named.
    let empty = Site::new().await;
    let error = import(&empty, &directory, "acme", "mirror", Mapping::default())
        .await
        .expect_err("there is nothing to import into");
    assert!(
        error.to_string().contains("create the proxy repository"),
        "{error}"
    );
}

#[tokio::test]
async fn crossing_owners_needs_the_explicit_mapping() {
    let origin = Site::new().await;
    origin.create_proxy("acme", "mirror", "central").await;
    let digest = origin
        .resolve("acme", "mirror", "tools/a.bin", b"tool a")
        .await;
    origin
        .receipt(
            "acme",
            "mirror",
            "tools/a.bin",
            &digest,
            6,
            ReceiptOutcome::Fetched,
            "central",
            1,
        )
        .await;
    let directory = export(&origin, "out", "acme", "mirror").await;

    let destination = Site::new().await;
    destination
        .create_proxy("umbrella", "mirror", "central")
        .await;

    // Without the explicit flag, the bundle's account is not the destination's.
    let error = import(
        &destination,
        &directory,
        "umbrella",
        "mirror",
        Mapping::default(),
    )
    .await
    .expect_err("the crossing is refused");
    assert!(error.to_string().contains("mapping"), "{error}");

    // With the flag, the import is a decision of the destination: the
    // receipts are left in the name of the importing account.
    let report = import(
        &destination,
        &directory,
        "umbrella",
        "mirror",
        Mapping {
            allow: true,
            overwrite: false,
        },
    )
    .await
    .expect("the mapped import runs");
    assert_eq!(report.imported, 1);
    let receipts = destination
        .provenance
        .receipts_of("umbrella", "mirror", "tools/a.bin")
        .await
        .expect("the ledger answers");
    assert_eq!(receipts[0].account, "umbrella");
    assert_eq!(receipts[0].repository, "mirror");
    assert_eq!(receipts[0].outcome, ReceiptOutcome::Imported);
    assert_eq!(
        destination
            .blobs()
            .get(&digest)
            .await
            .expect("the bytes verify"),
        b"tool a"
    );
}

#[tokio::test]
async fn a_route_collision_is_explained_and_only_broken_on_request() {
    let origin = Site::new().await;
    origin.create_proxy("acme", "mirror", "central").await;
    let digest = origin
        .resolve("acme", "mirror", "tools/a.bin", b"tool a")
        .await;
    origin
        .receipt(
            "acme",
            "mirror",
            "tools/a.bin",
            &digest,
            6,
            ReceiptOutcome::Fetched,
            "central",
            1,
        )
        .await;
    let directory = export(&origin, "out", "acme", "mirror").await;

    let destination = Site::new().await;
    destination.create_proxy("acme", "mirror", "central").await;
    let displaced = destination
        .resolve("acme", "mirror", "tools/a.bin", b"different tool a")
        .await;

    let error = import(
        &destination,
        &directory,
        "acme",
        "mirror",
        Mapping::default(),
    )
    .await
    .expect_err("the route belongs to other content");
    assert!(error.to_string().contains("overwrite"), "{error}");
    assert_eq!(
        destination
            .store
            .asset("acme", "mirror", "tools/a.bin")
            .await
            .expect("the old pointer stays")
            .digest,
        displaced
    );

    let report = import(
        &destination,
        &directory,
        "acme",
        "mirror",
        Mapping {
            allow: false,
            overwrite: true,
        },
    )
    .await
    .expect("the overwrite import runs");
    assert_eq!(report.imported, 1);
    assert_eq!(
        destination
            .store
            .asset("acme", "mirror", "tools/a.bin")
            .await
            .expect("the route moved")
            .digest,
        digest
    );
}
