//! HSP-207: la lectura de la capsula de release sobre la autoridad de
//! grafo — lo que la capsula firmada afirma, el grafo lo repite byte a
//! byte; cada lectura esta acotada por la clave del manifiesto y una
//! capsula distractor no se alcanza; la revocacion es un estado visible
//! junto al historial, nunca un silencio que reescriba lo firmado.
//!
//! Va gateado en HODEI_GRAPH_IT_ENDPOINT como todo IT de la pila, y
//! SERIALIZADO: el alpha es compartido con otros workers (HSP-192 GC,
//! HSP-201 OCI), asi que las aserciones son SIEMPRE de membrecia sobre
//! claves propias del tenant `hsp207-*` de esta ejecucion — nunca censos
//! globales. El formato de la capsula (manifiestos, recibos, firma,
//! absence explicita) es el contrato offline de HSP-126: este IT no lo
//! reescribe, lo usa como oracle de paridad export <-> grafo.
//!
//! Lo que prueba, contra el motor real:
//!
//! - **Paridad AC1**: tras `ingest`, `read_capsule` devuelve los bytes
//!   exactos del layout (el manifiesto de imagen que la firma firma, el
//!   `release.json`, el estado de confianza, la armadura), el mismo
//!   digest de release, los mismos paths con sus recibos y su decision
//!   historica, las mismas ausencias con su motivo — y la firma detached
//!   sigue verificando sobre los bytes que el grafo guarda.
//! - **Frontera AC2**: una capsula distractor (otro digest de imagen,
//!   otra cuenta y repositorio, porque el alpha es compartido) se ingiere igual; leer la primera
//!   responde exactamente sus paths, y el path del distractor no aparece
//!   jamas en su respuesta. Las consultas del modulo son `eq` exacto
//!   acotado — la prueba de forma vive en las unidades.
//! - **Revocacion visible AC1+AC2**: revocada la decision en el libro de
//!   confianza tras el export, la capsula del grafo conserva intacta la
//!   decision firmada y `read_capsule_with_trust` la reporta `Revoked`;
//!   re-promover con otra decision la reporta `Superseded` con la
//!   decision vigente nombrada. La historia no se reescribe; la verdad
//!   de hoy se lee al lado.
//! - **Inmutabilidad**: reingerir los mismos bytes es un replay que no
//!   duplica; un layout que no verifica no toca el grafo; una clave sin
//!   capsula se rechaza, no se responde vacia.

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

/// Un registro con su catalogo y su almacen, en un directorio propio.
///
/// HSP-314 F4: el catalogo y el libro de recibos son los del grafo (antes
/// eran el motor embebido, que es efimero por prueba). El alpha es
/// compartido y persistente, asi que cada `Site` lleva su PROPIA cuenta y
/// su PROPIO repositorio: nada de una instancia toca las filas de otra, ni
/// las de una corrida anterior. El libro de decisiones sigue siendo el
/// espejo en memoria (`MirrorLedger`): el de confianza vive en otra slice.
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

    /// Lo que hace una resolucion de proxy: guardar y publicar.
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

    /// El recibo firmado que confirma una resolucion.
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
        // El destino es otro `Site` (otra cuenta: el alpha es compartido y
        // cada sitio lleva la suya), asi que la importacion cruza duenos y
        // lo declara de forma explicita, como pide el contrato de HSP-126.
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

/// El digest del manifiesto de imagen announced por el indice, como lo
/// lee el propio indice (el orden del oraculo HSP-126: el manifest de la
/// capsula es el announced).
fn index_image_digest(layout: &Path) -> String {
    read_json(&layout.join("index.json"))["manifests"][0]["digest"]
        .as_str()
        .expect("a digest")
        .to_string()
}

/// Los bytes exactos del blob que el indice anuncia para un digest.
fn layout_blob(layout: &Path, digest: &str) -> Vec<u8> {
    std::fs::read(layout.join(LAYOUT_BLOBS).join(blob_hex(digest))).expect("the layout blob")
}

/// El digest SHA-256 en la forma del layout: `sha256:` + hex minuscula.
fn digest_of(bytes: &[u8]) -> String {
    use sha2::Digest as _;
    use std::fmt::Write as _;
    let mut digest = String::from("sha256:");
    for byte in sha2::Sha256::digest(bytes) {
        write!(&mut digest, "{byte:02x}").expect("String");
    }
    digest
}

/// El espejo de concesiones que la lectura con confianza consulta: el
/// libro de decisiones del grafo vive en otra slice; aqui lo que se
/// prueba es el overlay — la capsula conserva su historial y el ledger
/// responde del presente.
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

/// El tenant de esta ejecucion: `hsp207-` + nanos + contador, como en
/// los ITs vecinos. Nada de una prueba toca el espacio de claves de otra.
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

/// El directorio de evidencia: `target/uat/hsp207/`.
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

/// Cuantos nodos hay bajo una clave exacta del grafo, para cualquier
/// predicado propio de esta slice: la comprobacion es de MEMBRECIA de
/// claves propias — nunca un censo global sobre el alpha compartido.
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

/// El paquete del escenario central: una ruta con decision (`tools/a.bin`),
/// una confirmada sin decision (`tools/b.bin`) y una retenida (`tools/c.bin`)
/// — la absence explicita forma parte del contrato.
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
    // c.bin no tiene recibo: se retiene y la capsula lo dice.
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

/// Concede el digest con la decision nombrada, para el overlay de
/// vigencia que lee `read_capsule_with_trust`.
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

/// AC1: la lectura del grafo es el layout, byte a byte, con la firma
/// viva. AC2: acotada por la clave del manifiesto, con el distractor
/// presente e inalcanzable desde la otra capsula.
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

    // El digest que el modulo nombra es el que el indice anuncia y el
    // que la firma firma: la misma clave en las dos puertas.
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

    // Membrecia propia: ESTA capsula una vez, ESTE path una vez.
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

    // AC1 — paridad byte a byte con el layout offline.
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
    // La firma detached verifica sobre los bytes que el grafo guarda:
    // la capsula del grafo es la capsula firmada, no una traduccion.
    graph
        .verify_signature(&signer)
        .expect("the signature holds over the graph's bytes");

    // El historial y la ausencia explicita, intactos.
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

    // AC2 — el distractor: otra capsula firmada, mismo tenant y mismo
    // repositorio. Se ingiere bajo su propia clave; leer la primera
    // jamas alcanza su path.
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

    // La clave del distractor esta viva y ES la suya: la frontera es de
    // membrecia — cada capsula responde de su clave, nada se fusiona.
    let second = store
        .read_capsule(&other_digest)
        .await
        .expect("the distractor reads by its own key");
    assert_eq!(second.assets.len(), 1);
    assert_eq!(second.assets[0].asset_path, "tools/z.bin");
    assert!(second.withheld.is_empty());
    assert_ne!(second.image_digest, first.image_digest);

    // El contrato offline de HSP-126 sigue vivo con el grafo delante:
    // el MISMO paquete exportado se importa intacto en otro destino.
    // Leer la capsula en el grafo no cambia como se ve un paquete ya
    // exportado — la paridad export <-> grafo es additive.
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

/// AC1 + AC2 — la revocacion no se esconde y no reescribe: la capsula
/// conserva su decision firmada; el ledger dice donde esta hoy.
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

    // Hoy la decision vigente ES la de la capsula: Active.
    let live = store
        .read_capsule_with_trust(&image_digest, &ledger, now)
        .await
        .expect("the capsule reads with trust");
    assert_eq!(
        live.decision_state_of("tools/a.bin"),
        Some(&DecisionState::Active),
        "the grant backs the digest"
    );

    // Se revoca tras el export. La capsula NO se reescribe: el byte
    // del release sigue siendo el byte firmado.
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
    // El historial intacto: la decision sigue nombrada, con su actor.
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
    // Sin decision historica, sin estado que revocar: NoDecision.
    assert_eq!(
        after.decision_state_of("tools/b.bin"),
        Some(&DecisionState::NoDecision)
    );

    // Re-promocion con OTRA decision: la capsula conserva la suya y el
    // estado nombra la vigente — Superseded, jamas la decision antigua.
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

/// Inmutabilidad del registro: el replay no duplica; un layout que no
/// verifica no toca el grafo; una clave sin capsula se rechaza.
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

    // El mismo layout otra vez: replay. Ni la capsula ni sus filas
    // duplican (membrecia exacta por clave propia, nunca censo).
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

    // Un layout cuya capsula de imagen no es lo que el indice anuncia
    // no entra: el gate offline del ingest lo caza ANTES de escribir y
    // el grafo queda intacto.
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

    // Y una clave inexistente se responde como Refused, no como vacio:
    // leer algo que no se ingirio es un error, nunca una capsula muda.
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

/// El manifest de la capsula es el announced por el indice; el de la
/// firma no entra en la clave: esta prueba lo deja dicho para el lector.
#[test]
fn a_layout_that_is_not_there_is_refused_not_invented() {
    // La puerta de entrada del grafo es offline: un layout que no existe
    // no produce una capsula vacia, produce Unsound. Nada de lo que el
    // grafo guarda puede descansar en un layout que no se verifico.
    let error = capsule_image_digest(Path::new("/this/path/does/not/exist"))
        .expect_err("a missing layout is an error");
    assert!(
        matches!(error, CapsuleGraphError::Unsound(_)),
        "missing layout: {error}"
    );
}
