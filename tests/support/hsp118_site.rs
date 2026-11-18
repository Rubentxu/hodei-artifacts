// Extraido literal de tests/hsp118_bundle_contract.rs: una capsula que
// no se exporte desde un paquete real no prueba nada. Imports y
// constantes los pone el test que incluye este andamiaje.
async fn open(path: &std::path::Path) -> Surreal<Db> {
    let mut last = String::new();
    for _ in 0..20 {
        match Surreal::new::<RocksDb>(path).await {
            Ok(db) => {
                db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
                return db;
            }
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    panic!("the database could not be opened: {last}");
}

/// Un registro con su catalogo y su almacen, en un directorio propio.
struct Site {
    tmp: tempfile::TempDir,
    store: SurrealCatalogStore,
    cas: std::path::PathBuf,
}

impl Site {
    async fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tmp");
        let store = SurrealCatalogStore::with_authorizer(
            open(&tmp.path().join("db")).await,
            Arc::new(AllowAllAuthorizer),
        );
        store.initialize().await.expect("initialize");
        let cas = tmp.path().join("cas");
        Site { tmp, store, cas }
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

    #[allow(dead_code)] // compartido: no todo consumidor crea repositorios hosted
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

    /// Lo que hace una resolucion de proxy: guardar, publicar y firmar el
    /// recibo que la confirma.
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
        self.store
            .record(&ProvenanceReceipt {
                id: format!(
                    "prv-{path}-{upstream_id}-{config_version}-{}",
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
        &site.store,
        &site.store,
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
        &site.store,
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
