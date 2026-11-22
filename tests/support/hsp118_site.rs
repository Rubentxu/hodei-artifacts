// Extracted verbatim from tests/hsp118_bundle_contract.rs: a capsule that
// is not exported from a real package proves nothing. Imports and
// constants are provided by the test that includes this scaffolding.
/// A registry with its catalog and its store, in its own directory.
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

    #[allow(dead_code)] // shared: not every consumer creates hosted repositories
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


async fn export_with(
    site: &Site,
    name: &str,
    signer: Option<&MetadataSigner>,
) -> std::path::PathBuf {
    let to = site.tmp.path().join(name);
    let blobs = site.blobs();
    let report = bundles::export(
        &site.store,
        &site.provenance,
        &site.trust,
        &blobs,
        "acme",
        "mirror",
        &to,
        signer,
    )
    .await
    .expect("the export runs");
    report.path
}

// compartido: el pasaporte de HSP-160 exporta pero no reimporta en destino
#[allow(dead_code)]
async fn import_into(
    site: &Site,
    directory: &std::path::Path,
    account: &str,
    repository: &str,
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
        Mapping::default(),
        "operator",
        "corr-capsule",
    )
    .await
}
