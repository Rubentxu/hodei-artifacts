//! El contrato del catálogo durable (HAR-02).
//!
//! Las pruebas van contra SurrealDB de verdad, y contra el motor
//! **persistente** en todas ellas, porque un catálogo que solo funciona en
//! memoria no es un catálogo, y porque el motor en memoria no arbitra dos
//! escrituras concurrentes del mismo registro: las dos dicen que han
//! escrito y la última gana en silencio.
//!
//! Los casos son los de la UAT `CAT-01..CAT-03` y los cinco criterios de
//! aceptación de HSP-96.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use hodei_catalog::{
    AccessDecision, AccessRequest, AllowAllAuthorizer, AssetRecord, BlobIntegrityProbe,
    BlobLocation, BlobProbeError, CatalogAction, CatalogAuthorizer, CatalogError, CatalogReader,
    CatalogWriter, DigestScope, MutableTag, PackageCoordinates, PackageVersion, Publication,
    PublishOutcome, RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
    repository_hrn,
};
use hodei_catalog_storage::{SCHEMA_VERSION, SurrealCatalogStore};
use hodei_evidence::ContentDigest;
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;

/// Una base en disco, con su namespace y su base de datos.
///
/// Las pruebas van contra el motor **persistente**, y no por casualidad:
/// el motor en memoria no arbitra dos escrituras concurrentes del mismo
/// registro (las dos dicen que han escrito y la última gana en silencio),
/// mientras que el persistente detecta el conflicto de escritura y
/// rechaza al que pierde. Una garantía que solo se cumple en un motor no
/// es una garantía del catálogo, es una casualidad del motor, así que las
/// pruebas se hacen contra el que se despliega.
async fn open_store(path: &std::path::Path) -> Surreal<surrealdb::engine::local::Db> {
    open_disk(path).await
}

/// Una base en disco, con su namespace y su base de datos.
///
/// RocksDB guarda un bloqueo en el directorio y lo suelta cuando el
/// proceso que lo abrió se deshace, pero lo suelta después: el primer
/// intento de reabrir puede encontrar el bloqueo aún puesto, y eso no es
/// un fallo del reinicio sino una carrera del motor.
async fn open_disk(path: &std::path::Path) -> Surreal<surrealdb::engine::local::Db> {
    let mut last = String::new();
    for _attempt in 0..20 {
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
    panic!("the database at {path:?} could not be opened: {last}");
}

/// Un almacén que deja pasar a las cuentas de la lista.
fn store_for(db: Surreal<surrealdb::engine::local::Db>) -> SurrealCatalogStore {
    SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer))
}

/// Un almacén con un decisor propio.
fn store_with(
    db: Surreal<surrealdb::engine::local::Db>,
    authorizer: Arc<dyn CatalogAuthorizer>,
) -> SurrealCatalogStore {
    SurrealCatalogStore::with_authorizer(db, authorizer)
}

/// El decisor que solo deja pasar a las cuentas de la lista.
struct ListedAccounts {
    allowed: Vec<String>,
}

#[async_trait]
impl CatalogAuthorizer for ListedAccounts {
    async fn authorize(&self, request: &AccessRequest) -> Result<AccessDecision, CatalogError> {
        if self.allowed.contains(&request.principal_account) {
            Ok(AccessDecision::Allow)
        } else {
            Ok(AccessDecision::Deny(format!(
                "the account {} is not on the list",
                request.principal_account
            )))
        }
    }
}

/// La sonda que dice la longitud que tiene cada blob.
///
/// Indexa por la **clave** con la que el CAS guarda los bytes, y no por el
/// digest: en un ámbito particionado el mismo digest vive en otra ranura,
/// y una sonda que solo conociera el digest daría por roto un blob que
/// está sano.
///
/// Además apunta donde se le preguntó, para que se pueda comprobar que la
/// pregunta lleva la ubicación y no solo el digest.
#[derive(Default)]
struct KnownLengths {
    lengths: HashMap<String, u64>,
    asked: std::sync::Mutex<Vec<String>>,
}

impl KnownLengths {
    /// Una sonda que solo conoce estos blobs.
    fn of(lengths: HashMap<String, u64>) -> Self {
        Self {
            lengths,
            asked: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// La clave de la última pregunta.
    fn last_asked(&self) -> Option<String> {
        self.asked
            .lock()
            .ok()
            .and_then(|asked| asked.last().cloned())
    }
}

#[async_trait]
impl BlobIntegrityProbe for KnownLengths {
    async fn length_of(&self, location: &BlobLocation) -> Result<u64, BlobProbeError> {
        let key = location.blob_key();
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(key.clone());
        }
        self.lengths
            .get(&key)
            .copied()
            .ok_or(BlobProbeError::Missing)
    }
}

fn descriptor(
    account: &str,
    repo_key: &str,
    format: RepositoryFormat,
    mode: RepositoryMode,
    scope: DigestScope,
) -> RepositoryDescriptor {
    RepositoryDescriptor {
        hrn: repository_hrn("hodei", "artifacts", "eu", account, repo_key),
        account: account.to_string(),
        repo_key: repo_key.to_string(),
        format,
        mode,
        config: RepositoryConfig::new(scope),
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

fn publication(
    repository: &str,
    asset_path: &str,
    package_name: &str,
    package_version: &str,
    content: &[u8],
) -> Publication {
    Publication {
        repository: repository.to_string(),
        asset_path: asset_path.to_string(),
        coordinates: PackageCoordinates {
            package_name: package_name.to_string(),
            package_version: package_version.to_string(),
        },
        digest: ContentDigest::of_bytes(content),
        size_bytes: content.len() as u64,
        tags: Vec::new(),
        actor: "hrn:hodei:iam:acme:User/alice".to_string(),
        correlation_id: "corr-har02".to_string(),
    }
}

fn with_tag(mut publication: Publication, tag: &str) -> Publication {
    publication.tags = vec![tag.to_string()];
    publication
}

// ---------------------------------------------------------------------------
// AC1: la identidad del repositorio.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_repository_declares_account_key_format_mode_and_versioned_config() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");

    let created = store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    assert_eq!(created.account, "acme");
    assert_eq!(created.repo_key, "releases");
    assert_eq!(created.format, RepositoryFormat::Raw);
    assert_eq!(created.mode, RepositoryMode::Hosted);
    assert_eq!(created.config.version, 1);
    assert_eq!(created.config.digest_scope, DigestScope::Global);
    assert_eq!(
        created.hrn.to_string(),
        "hrn:hodei:artifacts:eu:acme:repository/releases"
    );

    // Y se lee igual que se escribió.
    let read = store
        .repository("acme", "releases")
        .await
        .expect("read repository");
    assert_eq!(read.repo_key, created.repo_key);
    assert_eq!(read.format, created.format);
    assert_eq!(read.mode, created.mode);
    assert_eq!(read.config.version, created.config.version);
    assert_eq!(read.hrn, created.hrn);
}

#[tokio::test]
async fn a_mode_this_release_does_not_serve_is_refused_clearly() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");

    for mode in [RepositoryMode::Proxy, RepositoryMode::Group] {
        let refused = store
            .create_repository(
                "acme",
                descriptor(
                    "acme",
                    "releases",
                    RepositoryFormat::Raw,
                    mode,
                    DigestScope::Global,
                ),
            )
            .await
            .expect_err("the mode is not served");
        assert_eq!(refused, CatalogError::ModeNotImplemented { mode });
        // Y el mensaje dice cuál es el modo, para que el que lo pidió sepa
        // que falta.
        assert!(refused.to_string().contains(mode.as_str()));
    }

    // Y no se ha creado ningún repositorio con ese modo.
    assert!(matches!(
        store.repository("acme", "releases").await,
        Err(CatalogError::Missing { .. })
    ));
}

#[tokio::test]
async fn the_config_version_rises_when_the_config_changes() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let mut config = RepositoryConfig::new(DigestScope::AccountPartitioned);
    config.version = 99;
    let updated = store
        .update_config("acme", "releases", config)
        .await
        .expect("update config");

    // La versión la pone el catálogo, no el cliente: subirla es lo que
    // significa que la configuración es otra.
    assert_eq!(updated.config.version, 2);
    assert_eq!(updated.config.digest_scope, DigestScope::AccountPartitioned);

    let read = store.repository("acme", "releases").await.expect("read");
    assert_eq!(read.config.version, 2);
    assert_eq!(read.config.digest_scope, DigestScope::AccountPartitioned);
}

// ---------------------------------------------------------------------------
// AC2: el ámbito del digest, el asset y la etiqueta mutable.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_digest_scope_is_the_policy_of_the_repository() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");

    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "shared",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create shared");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "private",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::AccountPartitioned,
            ),
        )
        .await
        .expect("create private");

    let content = b"the same bytes";
    store
        .publish(
            "acme",
            publication("shared", "a.bin", "pkg", "1.0", content),
        )
        .await
        .expect("publish shared");
    store
        .publish(
            "acme",
            publication("private", "a.bin", "pkg", "1.0", content),
        )
        .await
        .expect("publish private");

    let shared = store
        .reference("acme", "shared", "a.bin")
        .await
        .expect("shared reference");
    let private = store
        .reference("acme", "private", "a.bin")
        .await
        .expect("private reference");

    assert_eq!(shared.scope, DigestScope::Global);
    assert_eq!(private.scope, DigestScope::AccountPartitioned);
    assert_eq!(shared.digest, private.digest);

    // Y la política decide donde viven los bytes.
    assert_eq!(
        shared.scope.blob_key("acme", &shared.digest),
        shared.digest.value
    );
    assert_eq!(
        private.scope.blob_key("acme", &private.digest),
        format!("acme/{}", private.digest.value)
    );
    assert_ne!(
        shared.scope.blob_key("acme", &shared.digest),
        private.scope.blob_key("acme", &private.digest)
    );
}

#[tokio::test]
async fn an_asset_is_an_account_a_repository_and_a_path() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let content = b"the artifact";
    let published = store
        .publish(
            "acme",
            publication("releases", "path/to/file.bin", "pkg", "1.2.3", content),
        )
        .await
        .expect("publish");
    assert!(published.created);

    let asset: AssetRecord = store
        .asset("acme", "releases", "path/to/file.bin")
        .await
        .expect("read asset");

    assert_eq!(asset.account, "acme");
    assert_eq!(asset.repository, "releases");
    assert_eq!(asset.asset_path, "path/to/file.bin");
    assert_eq!(asset.coordinates.package_name, "pkg");
    assert_eq!(asset.coordinates.package_version, "1.2.3");
    assert_eq!(asset.digest, ContentDigest::of_bytes(content));
    assert_eq!(asset.size_bytes, content.len() as u64);

    // Y la versión que apunta es la que se publicó.
    let version: PackageVersion = store
        .version(
            "acme",
            "releases",
            &PackageCoordinates {
                package_name: "pkg".to_string(),
                package_version: "1.2.3".to_string(),
            },
        )
        .await
        .expect("read version");
    assert_eq!(version.digest, asset.digest);
    assert_eq!(version.size_bytes, asset.size_bytes);
}

#[tokio::test]
async fn a_mutable_tag_moves_without_touching_the_blob() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let first = b"the first version";
    let second = b"the second version";
    store
        .publish(
            "acme",
            with_tag(
                publication("releases", "app-1.0.bin", "pkg", "1.0", first),
                "latest",
            ),
        )
        .await
        .expect("publish first");
    store
        .publish(
            "acme",
            publication("releases", "app-2.0.bin", "pkg", "2.0", second),
        )
        .await
        .expect("publish second");

    let tag_before: MutableTag = store
        .tag("acme", "releases", "latest")
        .await
        .expect("read tag");
    assert_eq!(tag_before.coordinates.package_version, "1.0");

    // Mover la etiqueta.
    let moved = store
        .move_tag(
            "acme",
            "releases",
            "latest",
            PackageCoordinates {
                package_name: "pkg".to_string(),
                package_version: "2.0".to_string(),
            },
        )
        .await
        .expect("move tag");
    assert_eq!(moved.coordinates.package_version, "2.0");

    // La versión que se queda atrás conserva su digest y sus bytes.
    let old = store
        .version(
            "acme",
            "releases",
            &PackageCoordinates {
                package_name: "pkg".to_string(),
                package_version: "1.0".to_string(),
            },
        )
        .await
        .expect("read old version");
    assert_eq!(old.digest, ContentDigest::of_bytes(first));
    assert_eq!(old.size_bytes, first.len() as u64);

    // Y el blob de la versión que se queda atrás sigue intacto y sigue
    // referenciado: la etiqueta no toca el blob.
    let reference = store
        .reference("acme", "releases", "app-1.0.bin")
        .await
        .expect("read reference");
    assert_eq!(reference.digest, ContentDigest::of_bytes(first));
    assert_eq!(reference.size_bytes, first.len() as u64);
}

#[tokio::test]
async fn moving_an_asset_repoints_the_path_and_its_blob_reference() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let first = b"the first content";
    let second = b"the second content";
    store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", first),
        )
        .await
        .expect("publish first");
    store
        .publish(
            "acme",
            publication("releases", "other.bin", "pkg", "2.0", second),
        )
        .await
        .expect("publish second");

    // Publicar en una ruta ocupada es un conflicto: el camino ya dice algo.
    let refused = store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "3.0", second),
        )
        .await
        .expect_err("the path is taken");
    assert!(
        matches!(refused, CatalogError::Conflict { kind: "asset", .. }),
        "unexpected error: {refused:?}"
    );

    // Moverlo es explícito, y mueve el asset y su referencia a la vez.
    let moved = store
        .move_asset(
            "acme",
            "releases",
            "app.bin",
            PackageCoordinates {
                package_name: "pkg".to_string(),
                package_version: "2.0".to_string(),
            },
        )
        .await
        .expect("move asset");
    assert_eq!(moved.coordinates.package_version, "2.0");
    assert_eq!(moved.digest, ContentDigest::of_bytes(second));

    // La referencia apunta al mismo contenido que el asset: si no, la
    // descarga serviría un blob y la recolección guardaría otro.
    let reference = store
        .reference("acme", "releases", "app.bin")
        .await
        .expect("read reference");
    assert_eq!(reference.digest, ContentDigest::of_bytes(second));
    assert_eq!(reference.size_bytes, second.len() as u64);

    // Y la versión anterior sigue intacta: mover un índice no reescribe
    // un contenido.
    let old = store
        .version(
            "acme",
            "releases",
            &PackageCoordinates {
                package_name: "pkg".to_string(),
                package_version: "1.0".to_string(),
            },
        )
        .await
        .expect("read old version");
    assert_eq!(old.digest, ContentDigest::of_bytes(first));
}

// ---------------------------------------------------------------------------
// AC3: la escritura es transaccional y no sobrescribe en silencio.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn a_second_incompatible_publication_is_a_conflict_and_does_not_overwrite() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let first = b"the first content";
    let second = b"a different content";
    store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", first),
        )
        .await
        .expect("publish first");

    // La misma versión con otro contenido es un conflicto: una versión
    // publicada no cambia.
    let refused = store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", second),
        )
        .await
        .expect_err("the version is immutable");
    assert!(
        matches!(
            refused,
            CatalogError::Conflict {
                kind: "version",
                ..
            }
        ),
        "unexpected error: {refused:?}"
    );

    // Y otra versión en la misma ruta también es un conflicto: la ruta ya
    // apunta a algo.
    let refused = store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "2.0", second),
        )
        .await
        .expect_err("the path already points somewhere");
    assert!(
        matches!(refused, CatalogError::Conflict { kind: "asset", .. }),
        "unexpected error: {refused:?}"
    );

    // Y nada de lo que se publicó cambió.
    let asset = store
        .asset("acme", "releases", "app.bin")
        .await
        .expect("read asset");
    assert_eq!(asset.digest, ContentDigest::of_bytes(first));
    assert_eq!(asset.coordinates.package_version, "1.0");
}

#[tokio::test]
async fn the_same_publication_twice_is_idempotent() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let content = b"the same content";
    let first = store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", content),
        )
        .await
        .expect("publish");
    assert!(first.created);

    let second = store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", content),
        )
        .await
        .expect("publish again");
    assert!(
        !second.created,
        "the second publication did not create anything"
    );
    assert_eq!(first.asset.digest, second.asset.digest);
    assert_eq!(first.version.digest, second.version.digest);
}

#[tokio::test]
async fn a_publication_that_fails_leaves_nothing_behind() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    // La etiqueta `latest` ya apunta a la versión 1.0.
    store
        .publish(
            "acme",
            with_tag(
                publication("releases", "app.bin", "pkg", "1.0", b"first"),
                "latest",
            ),
        )
        .await
        .expect("publish first");

    // Ahora se publica otra versión en otra ruta, con la misma etiqueta. La
    // etiqueta ya apunta a otro sitio, así que la publicación no puede
    // hacerse: y al no poder hacerse, no deja ni el asset ni la versión.
    let refused = store
        .publish(
            "acme",
            with_tag(
                publication("releases", "other.bin", "pkg", "2.0", b"second"),
                "latest",
            ),
        )
        .await
        .expect_err("the tag already points somewhere");
    assert!(
        matches!(refused, CatalogError::Conflict { kind: "tag", .. }),
        "unexpected error: {refused:?}"
    );

    // Ni el asset ni la versión existen: la transacción se fue entera.
    assert!(matches!(
        store.asset("acme", "releases", "other.bin").await,
        Err(CatalogError::Missing { .. })
    ));
    assert!(matches!(
        store
            .version(
                "acme",
                "releases",
                &PackageCoordinates {
                    package_name: "pkg".to_string(),
                    package_version: "2.0".to_string(),
                }
            )
            .await,
        Err(CatalogError::Missing { .. })
    ));
    assert!(matches!(
        store.reference("acme", "releases", "other.bin").await,
        Err(CatalogError::Missing { .. })
    ));
}

// ---------------------------------------------------------------------------
// AC4: aislamiento por cuenta, unicidad, concurrencia y reinicio.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn two_accounts_with_the_same_path_do_not_see_each_other() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db.clone());
    store.initialize().await.expect("initialize");

    for account in ["acme", "globex"] {
        store
            .create_repository(
                account,
                descriptor(
                    account,
                    "releases",
                    RepositoryFormat::Raw,
                    RepositoryMode::Hosted,
                    DigestScope::Global,
                ),
            )
            .await
            .expect("create repository");
    }

    let acme_content = b"the content of acme";
    let globex_content = b"the content of globex";
    store
        .publish(
            "acme",
            publication("releases", "shared.bin", "pkg", "1.0", acme_content),
        )
        .await
        .expect("publish acme");
    store
        .publish(
            "globex",
            publication("releases", "shared.bin", "pkg", "1.0", globex_content),
        )
        .await
        .expect("publish globex");

    // Cada cuenta ve lo suyo y solo lo suyo.
    let acme = store
        .asset("acme", "releases", "shared.bin")
        .await
        .expect("read acme");
    let globex = store
        .asset("globex", "releases", "shared.bin")
        .await
        .expect("read globex");
    assert_eq!(acme.digest, ContentDigest::of_bytes(acme_content));
    assert_eq!(globex.digest, ContentDigest::of_bytes(globex_content));
    assert_ne!(acme.digest, globex.digest);

    // Y lo que una cuenta no tiene no existe para ella: no es un
    // `Denied` que revele que hay algo, ni un `Corrupt`.
    assert!(matches!(
        store.asset("acme", "releases", "only-globex.bin").await,
        Err(CatalogError::Missing { .. })
    ));

    // Los repositorios de una cuenta no se ven desde la otra, aunque
    // tengan la misma clave.
    assert!(store.repository("globex", "releases").await.is_ok());
    let other = descriptor(
        "acme",
        "only-acme",
        RepositoryFormat::Raw,
        RepositoryMode::Hosted,
        DigestScope::Global,
    );
    store
        .create_repository("acme", other)
        .await
        .expect("create acme only");
    assert!(matches!(
        store.repository("globex", "only-acme").await,
        Err(CatalogError::Missing { .. })
    ));
}

#[tokio::test]
async fn the_same_repository_key_twice_in_one_account_is_a_conflict() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");

    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let refused = store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Oci,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect_err("the key is taken");
    assert!(
        matches!(
            refused,
            CatalogError::Conflict {
                kind: "repository",
                ..
            }
        ),
        "unexpected error: {refused:?}"
    );
}

#[tokio::test]
async fn two_clients_publishing_at_the_same_time_leave_one_winner() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let left = Arc::new(store_for(db.clone()));
    let right = Arc::new(store_for(db));
    left.initialize().await.expect("initialize");
    right.initialize().await.expect("initialize");

    left.create_repository(
        "acme",
        descriptor(
            "acme",
            "releases",
            RepositoryFormat::Raw,
            RepositoryMode::Hosted,
            DigestScope::Global,
        ),
    )
    .await
    .expect("create repository");

    // Los dos clientes publican el MISMO contenido en la misma ruta.
    let content = b"the same content";
    let same_left = left.publish(
        "acme",
        publication("releases", "app.bin", "pkg", "1.0", content),
    );
    let same_right = right.publish(
        "acme",
        publication("releases", "app.bin", "pkg", "1.0", content),
    );
    let (same_left, same_right) = tokio::join!(same_left, same_right);

    let outcomes: Vec<Result<PublishOutcome, CatalogError>> = vec![same_left, same_right];
    let created = outcomes
        .iter()
        .filter(|result| matches!(result, Ok(outcome) if outcome.created))
        .count();
    let idempotent = outcomes
        .iter()
        .filter(|result| matches!(result, Ok(outcome) if !outcome.created))
        .count();
    assert_eq!(created, 1, "one client created the asset");
    assert_eq!(idempotent, 1, "the other one found it already there");

    // Ahora los dos clientes publican CONTENIDO DISTINTO en la misma ruta.
    // Las coordenadas son distintas a propósito: así cada uno tiene su
    // propia versión, y el que pierde se encuentra con que la versión se
    // puede escribir pero el asset de la ruta ya es del otro. Su
    // transacción se cae a mitad, y de ahí se ve si el motor deshace lo
    // que ya había escrito.
    let conflict_left = left.publish(
        "acme",
        publication("releases", "other.bin", "pkg", "2.0", b"the left content"),
    );
    let conflict_right = right.publish(
        "acme",
        publication("releases", "other.bin", "pkg", "3.0", b"the right content"),
    );
    let (conflict_left, conflict_right) = tokio::join!(conflict_left, conflict_right);

    let results: Vec<Result<PublishOutcome, CatalogError>> = vec![conflict_left, conflict_right];
    let winners = results.iter().filter(|result| result.is_ok()).count();
    let conflicts = results
        .iter()
        .filter(|result| matches!(result, Err(CatalogError::Conflict { .. })))
        .count();
    assert_eq!(winners, 1, "one client won the race");
    assert_eq!(
        conflicts, 1,
        "the other one got a conflict, not an overwrite"
    );

    // Y lo que quedó es el contenido del que ganó, no una mezcla.
    let asset = left
        .asset("acme", "releases", "other.bin")
        .await
        .expect("read asset");
    let digest = asset.digest.clone();
    assert!(
        digest == ContentDigest::of_bytes(b"the left content")
            || digest == ContentDigest::of_bytes(b"the right content"),
        "the asset holds a digest that nobody published"
    );

    // La referencia dice lo mismo que el asset, y la versión a la que
    // apunta existe y dice lo mismo. Si la transacción del perdedor se
    // hubiera quedado a medias, aquí aparecería una versión con un
    // contenido que no es el del asset.
    let reference = left
        .reference("acme", "releases", "other.bin")
        .await
        .expect("read reference");
    assert_eq!(reference.digest, asset.digest);
    let version = left
        .version("acme", "releases", &asset.coordinates)
        .await
        .expect("read version");
    assert_eq!(version.digest, asset.digest);

    // Y la versión del perdedor no está: su transacción se deshizo entera.
    let loser = PackageCoordinates {
        package_name: "pkg".to_string(),
        package_version: if asset.coordinates.package_version == "2.0" {
            "3.0".to_string()
        } else {
            "2.0".to_string()
        },
    };
    assert!(
        left.version("acme", "releases", &loser).await.is_err(),
        "the loser of the race left a version behind"
    );
}

/// La ruta de la base, que el proceso que escribe recibe del que lo lanza.
///
/// Solo la lee quien escribe. El proceso que lee no la necesita, y por eso
/// no la mira: si la mirase, una variable de entorno puesta por error en
/// la shell podría hacer que el proceso que verifica se creyera el que
/// escribe.
const RESTART_PATH: &str = "HAR02_CATALOG_RESTART_PATH";

/// Escribe el catálogo y sale.
///
/// Vive en un proceso aparte porque el reinicio que se quiere probar es un
/// reinicio de verdad: el bloqueo de la base lo suelta el sistema operativo
/// cuando el proceso muere, y abrir la base en el mismo proceso no
/// probaría eso.
async fn write_the_catalog(path: &std::path::Path) {
    let db = open_disk(path).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");
    store
        .publish(
            "acme",
            with_tag(
                publication("releases", "app.bin", "pkg", "1.0", b"the durable content"),
                "latest",
            ),
        )
        .await
        .expect("publish");
}

/// La fase de escritura del reinicio, en su propio proceso.
///
/// Es un test aparte, y no una rama del de reinicio, por una razón que
/// no es de estilo: si la fase de escritura fuera una rama, bastaría con
/// que la variable de entorno que la activa estuviera puesta en el
/// entorno de quien lanza la suite para que el proceso que verifica
/// escribiera y se saliera **sin comprobar nada**, y el test pasaría en
/// verde sin haber verificado nada. Con dos tests, el que verifica no
/// tiene rama que lo saltee, y el que escribe falla si se lo lanza sin
/// la ruta.
#[tokio::test]
#[ignore = "solo corre como el proceso hijo del test de reinicio"]
async fn the_catalog_is_written_for_the_restart() {
    let path = std::env::var(RESTART_PATH).expect(
        "this test only runs as the writing process of the restart test, \
         which passes it the path of the database",
    );
    write_the_catalog(std::path::Path::new(&path)).await;
}

#[tokio::test]
async fn the_catalog_survives_a_restart() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("db");

    // El proceso que escribe es otro proceso, y se espera a que termine.
    let binary = std::env::current_exe().expect("the test binary");
    let status = tokio::process::Command::new(binary)
        .arg("--exact")
        .arg("the_catalog_is_written_for_the_restart")
        .arg("--ignored")
        .arg("--nocapture")
        .env(RESTART_PATH, path.to_string_lossy().as_ref())
        .status()
        .await
        .expect("the writing process");
    assert!(status.success(), "the writing process failed: {status}");

    // Un proceso nuevo, sobre la misma base, ve lo mismo.
    let db = open_disk(&path).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");

    let repository = store
        .repository("acme", "releases")
        .await
        .expect("the repository survives the restart");
    assert_eq!(repository.repo_key, "releases");

    let asset = store
        .asset("acme", "releases", "app.bin")
        .await
        .expect("the asset survives the restart");
    assert_eq!(
        asset.digest,
        ContentDigest::of_bytes(b"the durable content")
    );
    assert_eq!(asset.coordinates.package_version, "1.0");

    let version = store
        .version(
            "acme",
            "releases",
            &PackageCoordinates {
                package_name: "pkg".to_string(),
                package_version: "1.0".to_string(),
            },
        )
        .await
        .expect("the version survives the restart");
    assert_eq!(
        version.digest,
        ContentDigest::of_bytes(b"the durable content")
    );

    let reference = store
        .reference("acme", "releases", "app.bin")
        .await
        .expect("the reference survives the restart");
    assert_eq!(
        reference.digest,
        ContentDigest::of_bytes(b"the durable content")
    );

    let tag = store
        .tag("acme", "releases", "latest")
        .await
        .expect("the tag survives the restart");
    assert_eq!(tag.coordinates.package_version, "1.0");
}

// ---------------------------------------------------------------------------
// AC5: la migración es idempotente y los tres estados de lectura se
// distinguen.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn the_schema_can_be_declared_twice() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("first initialize");
    store.initialize().await.expect("second initialize");
    assert_eq!(
        store.applied_schema_version().await.expect("version"),
        SCHEMA_VERSION
    );

    // Y los índices siguen en su sitio: el índice es lo que decide los
    // conflictos, así que perderlo sería perder la garantía.
    let created = store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    // Pedir el mismo repositorio otra vez no es un error: es la misma
    // respuesta. Lo que si es un error es pedir OTRO repositorio con la
    // misma clave, y ahí el índice decide.
    let again = store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("the same repository is the same repository");
    assert_eq!(again.repo_key, created.repo_key);
    assert_eq!(again.hrn, created.hrn);
    assert_eq!(again.config.version, created.config.version);

    let refused = store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Oci,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect_err("the index is still there");
    assert!(matches!(
        refused,
        CatalogError::Conflict {
            kind: "repository",
            ..
        }
    ));
}

#[tokio::test]
async fn missing_denied_and_corrupt_are_three_different_answers() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db.clone());
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");
    store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", b"the content"),
        )
        .await
        .expect("publish");

    // MISSING: no hay nada ahí.
    let missing = store
        .asset("acme", "releases", "nothing-here.bin")
        .await
        .expect_err("there is nothing there");
    assert!(
        matches!(missing, CatalogError::Missing { kind: "asset", .. }),
        "unexpected error: {missing:?}"
    );

    // DENIED: hay algo ahí y este principal no puede verlo. El decisor
    // decide antes de mirar la base, así que la respuesta no depende de
    // que el objeto exista o no.
    let denying = store_with(
        db.clone(),
        Arc::new(ListedAccounts {
            allowed: vec!["acme".to_string()],
        }),
    );
    let denied = denying
        .asset("globex", "releases", "app.bin")
        .await
        .expect_err("the account is not on the list");
    assert!(
        matches!(denied, CatalogError::Denied { .. }),
        "unexpected error: {denied:?}"
    );
    assert!(denied.to_string().contains("globex"));

    // CORRUPT: hay algo ahí y no cuadra. El asset dice un digest y la
    // versión a la que apunta dice otro.
    db.query(
        "UPDATE catalog_asset SET digest = $digest \
         WHERE account = 'acme' AND repository = 'releases' AND asset_path = 'app.bin'",
    )
    .bind(("digest", "0".repeat(64)))
    .await
    .expect("tamper")
    .check()
    .expect("tamper ok");

    let corrupt = store
        .asset("acme", "releases", "app.bin")
        .await
        .expect_err("the record does not add up");
    assert!(
        matches!(corrupt, CatalogError::Corrupt { kind: "asset", .. }),
        "unexpected error: {corrupt:?}"
    );
    assert!(corrupt.to_string().contains("digest"));
}

#[tokio::test]
async fn an_asset_whose_blob_is_not_the_declared_size_is_corrupt() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let content = b"the content";
    let digest = ContentDigest::of_bytes(content);

    let store = store_for(db.clone());
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");
    store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", content),
        )
        .await
        .expect("publish");

    // Con la sonda que dice la longitud de verdad, el asset está sano.
    let healthy = SurrealCatalogStore::with_authorizer(db.clone(), Arc::new(AllowAllAuthorizer))
        .with_probe(Arc::new(KnownLengths::of(HashMap::from([(
            digest.value.clone(),
            content.len() as u64,
        )]))));
    healthy
        .asset("acme", "releases", "app.bin")
        .await
        .expect("the blob is the size the catalog says");

    // Con una sonda que dice otra cosa, el registro no cuadra con los
    // bytes: eso es corrupto, y no "no existe".
    let lying = SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer)).with_probe(
        Arc::new(KnownLengths::of(HashMap::from([(
            digest.value.clone(),
            999,
        )]))),
    );
    let corrupt = lying
        .asset("acme", "releases", "app.bin")
        .await
        .expect_err("the blob is not the size the catalog says");
    assert!(
        matches!(corrupt, CatalogError::Corrupt { kind: "asset", .. }),
        "unexpected error: {corrupt:?}"
    );
}

#[tokio::test]
async fn a_store_without_an_authorizer_serves_nothing() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = SurrealCatalogStore::new(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect_err("there is no authorizer");
    let denied = store
        .asset("acme", "releases", "app.bin")
        .await
        .expect_err("there is no authorizer");
    assert!(
        matches!(denied, CatalogError::Denied { .. }),
        "unexpected error: {denied:?}"
    );
}

#[tokio::test]
async fn publishing_into_a_repository_that_does_not_exist_is_missing() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    let refused = store
        .publish(
            "acme",
            publication("nowhere", "app.bin", "pkg", "1.0", b"content"),
        )
        .await
        .expect_err("the repository is not there");
    assert!(
        matches!(
            refused,
            CatalogError::Missing {
                kind: "repository",
                ..
            }
        ),
        "unexpected error: {refused:?}"
    );
}

#[tokio::test]
async fn the_catalog_does_not_publish_what_it_was_not_asked_to_publish() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    // Una publicación sin etiqueta no crea ninguna etiqueta.
    store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", b"content"),
        )
        .await
        .expect("publish");
    assert!(matches!(
        store.tag("acme", "releases", "latest").await,
        Err(CatalogError::Missing { .. })
    ));

    // Y una publicación con etiqueta crea esa etiqueta y solo esa.
    store
        .publish(
            "acme",
            with_tag(
                publication("releases", "other.bin", "pkg", "2.0", b"other content"),
                "latest",
            ),
        )
        .await
        .expect("publish with tag");
    let tag = store
        .tag("acme", "releases", "latest")
        .await
        .expect("read tag");
    assert_eq!(tag.coordinates.package_version, "2.0");
    assert!(matches!(
        store.tag("acme", "releases", "stable").await,
        Err(CatalogError::Missing { .. })
    ));
}

#[tokio::test]
async fn the_action_of_the_request_is_what_is_authorized() {
    // El decisor ve que se pide, no solo quien: una petición de lectura y
    // una de publicación no son la misma petición.
    struct Recorder {
        seen: std::sync::Mutex<Vec<CatalogAction>>,
    }

    #[async_trait]
    impl CatalogAuthorizer for Recorder {
        async fn authorize(&self, request: &AccessRequest) -> Result<AccessDecision, CatalogError> {
            self.seen.lock().expect("lock").push(request.action);
            Ok(AccessDecision::Allow)
        }
    }

    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let recorder = Arc::new(Recorder {
        seen: std::sync::Mutex::new(Vec::new()),
    });
    let store = store_with(db, recorder.clone());
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");
    store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", b"content"),
        )
        .await
        .expect("publish");
    store
        .asset("acme", "releases", "app.bin")
        .await
        .expect("read");

    let seen = recorder.seen.lock().expect("lock").clone();
    assert!(seen.contains(&CatalogAction::Configure));
    assert!(seen.contains(&CatalogAction::Publish));
    assert!(seen.contains(&CatalogAction::Read));
}

// ---------------------------------------------------------------------------
// Lo que la revisión independiente pidió que quedara demostrado.
// ---------------------------------------------------------------------------

/// El decisor que permite escribir pero no leer.
///
/// Es un decisor raro, y por eso sirve: comprueba que una escritura no
/// depende de un permiso de lectura que el que escribe no tiene por que
/// tener. Antes, escribir y releer por el camino público hacía que un
/// principal con permiso de escritura y sin permiso de lectura perdía su
/// escritura: el cambio ya estaba en la base y el error decía que no.
struct WritesOnly;

#[async_trait]
impl CatalogAuthorizer for WritesOnly {
    async fn authorize(&self, request: &AccessRequest) -> Result<AccessDecision, CatalogError> {
        match request.action {
            CatalogAction::Read => Ok(AccessDecision::Deny(
                "this principal may write but not read".to_string(),
            )),
            _ => Ok(AccessDecision::Allow),
        }
    }
}

/// Dos coordenadas que solo se distinguen por el separador no pueden
/// terminar en el mismo registro.
///
/// El identificador de un registro se construye con un separador. Si el
/// separador puede aparecer dentro de una parte, "pkg|1" + "0" y "pkg" +
/// "1|0" producen el mismo identificador, y la segunda publicación pisa
/// la primera en silencio.
#[tokio::test]
async fn two_names_that_only_differ_in_the_separator_do_not_collide() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    store
        .publish(
            "acme",
            publication("releases", "first.bin", "pkg|1", "0", b"the first one"),
        )
        .await
        .expect("publish the first one");
    store
        .publish(
            "acme",
            publication("releases", "second.bin", "pkg", "1|0", b"the second one"),
        )
        .await
        .expect("publish the second one");

    let first = store
        .version(
            "acme",
            "releases",
            &PackageCoordinates {
                package_name: "pkg|1".to_string(),
                package_version: "0".to_string(),
            },
        )
        .await
        .expect("read the first version");
    let second = store
        .version(
            "acme",
            "releases",
            &PackageCoordinates {
                package_name: "pkg".to_string(),
                package_version: "1|0".to_string(),
            },
        )
        .await
        .expect("read the second version");

    assert_eq!(first.digest, ContentDigest::of_bytes(b"the first one"));
    assert_eq!(second.digest, ContentDigest::of_bytes(b"the second one"));
}

/// Escribir no es leer.
///
/// Un principal que puede escribir pero no puede leer tiene que poder
/// escribir: si al terminar de escribir el catálogo vuelve a leer por el
/// camino público, la escritura se pierde en un `Denied` cuando el cambio
/// ya está en la base.
#[tokio::test]
async fn a_write_does_not_need_the_read_permission() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_with(db, Arc::new(WritesOnly));
    store.initialize().await.expect("initialize");

    let created = store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");
    assert_eq!(created.repo_key, "releases");

    let published = store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", b"the first one"),
        )
        .await
        .expect("publish");
    assert_eq!(published.asset.asset_path, "app.bin");

    // Y una segunda versión, para tener a donde mover el asset.
    store
        .publish(
            "acme",
            publication("releases", "other.bin", "pkg", "2.0", b"the second one"),
        )
        .await
        .expect("publish");

    let tag = store
        .move_tag(
            "acme",
            "releases",
            "latest",
            PackageCoordinates {
                package_name: "pkg".to_string(),
                package_version: "2.0".to_string(),
            },
        )
        .await
        .expect("move tag");
    assert_eq!(tag.tag_name, "latest");

    let moved = store
        .move_asset(
            "acme",
            "releases",
            "app.bin",
            PackageCoordinates {
                package_name: "pkg".to_string(),
                package_version: "2.0".to_string(),
            },
        )
        .await
        .expect("move asset");
    assert_eq!(moved.coordinates.package_version, "2.0");
}

/// Dos configuraciones distintas no pueden compartir un número de versión.
///
/// La versión de la configuración sirve justo para eso: lo que tiene el
/// cliente delante y lo que tiene el repositorio. Si dos reconfiguraciones
/// simultáneas se quedan con el mismo número y ajustes distintos, el
/// número ya no dice nada.
#[tokio::test]
async fn two_configurations_never_share_a_version() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = Arc::new(store_for(db));
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let mut left = RepositoryConfig::new(DigestScope::Global);
    left.config_settings = serde_json::json!({ "left": true });
    let mut right = RepositoryConfig::new(DigestScope::Global);
    right.config_settings = serde_json::json!({ "right": true });

    let left_store = Arc::clone(&store);
    let right_store = Arc::clone(&store);
    let (left_result, right_result) = tokio::join!(
        async move { left_store.update_config("acme", "releases", left).await },
        async move { right_store.update_config("acme", "releases", right).await },
    );

    let mut seen: Vec<(u64, serde_json::Value)> = Vec::new();
    for result in [&left_result, &right_result] {
        let descriptor = match result {
            Ok(descriptor) => descriptor,
            // Si una de las dos se queda fuera, que sea por conflicto: un
            // "el almacenamiento no está disponible" sería untrue.
            Err(error) => {
                assert!(
                    matches!(error, CatalogError::Conflict { .. }),
                    "unexpected error: {error:?}"
                );
                continue;
            }
        };
        let entry = (
            descriptor.config.version,
            descriptor.config.config_settings.clone(),
        );
        if let Some((_, other)) = seen.iter().find(|(version, _)| *version == entry.0) {
            assert_eq!(
                other, &entry.1,
                "two different configurations share the version {}",
                entry.0
            );
        }
        seen.push(entry);
    }

    // Y lo que quedó guardado es una de las dos, con su número. No puede
    // haber una tercera configuración escondida.
    let stored = store
        .repository("acme", "releases")
        .await
        .expect("read repository");
    assert!(
        seen.iter()
            .any(|(version, settings)| *version == stored.config.version
                && *settings == stored.config.config_settings),
        "the stored configuration is one that nobody asked for: {stored:?}"
    );
}

/// El ámbito de los digests no cambia en un repositorio que ya tiene
/// contenido.
///
/// Los bytes se publicaron bajo un ámbito, y ese es el ámbito en el que
/// siguen viviendo. Cambiarlo dejaría la política diciendo una cosa y los
/// bytes estando en otra.
#[tokio::test]
async fn the_scope_of_a_repository_with_content_cannot_change() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");
    store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", b"the content"),
        )
        .await
        .expect("publish");

    let refused = store
        .update_config(
            "acme",
            "releases",
            RepositoryConfig::new(DigestScope::AccountPartitioned),
        )
        .await
        .expect_err("the scope of a repository with content cannot change");
    assert!(
        matches!(refused, CatalogError::Conflict { .. }),
        "unexpected error: {refused:?}"
    );

    // Y el ámbito sigue siendo el de siempre.
    let repository = store
        .repository("acme", "releases")
        .await
        .expect("read repository");
    assert_eq!(repository.config.digest_scope, DigestScope::Global);
}

/// Publicar contenido que no está en el almacén de bytes no se publica.
///
/// Una referencia a un blob que no existe deja un camino que no lleva a
/// nada. El catálogo que afirma una integridad que no verifica es un
/// catálogo que miente.
#[tokio::test]
async fn the_content_has_to_be_in_the_store_before_it_is_published() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    // Una sonda que no conoce ningún blob.
    let store = store_with(db, Arc::new(AllowAllAuthorizer))
        .with_probe(Arc::new(KnownLengths::of(HashMap::new())));
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let refused = store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", b"the content"),
        )
        .await
        .expect_err("the content is not in the store");
    assert!(
        matches!(refused, CatalogError::InvalidRequest { .. }),
        "unexpected error: {refused:?}"
    );

    // Y no queda nada escrito: la publicación no llegó a empezar.
    assert!(
        store.asset("acme", "releases", "app.bin").await.is_err(),
        "the publication left an asset behind"
    );
}

/// La sonda se pregunta por la ubicación del contenido, no por su digest.
///
/// El mismo digest puede estar en la ranura global o en la particionada
/// de una cuenta. Preguntar por el digest a solo encontraría la primera,
/// y daría por roto un blob particionado que está sano.
#[tokio::test]
async fn the_probe_is_asked_where_the_content_lives() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let content = b"the content";
    let digest = ContentDigest::of_bytes(content);

    // El repositorio es particionado: sus blobs viven en "cuenta/digest".
    let probe = Arc::new(KnownLengths::of(HashMap::from([(
        format!("acme/{}", digest.value),
        content.len() as u64,
    )])));
    let store = store_with(db, Arc::new(AllowAllAuthorizer)).with_probe(probe.clone());
    store.initialize().await.expect("initialize");
    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::AccountPartitioned,
            ),
        )
        .await
        .expect("create repository");

    store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", content),
        )
        .await
        .expect("publish");

    assert_eq!(
        probe.last_asked(),
        Some(format!("acme/{}", digest.value)),
        "the probe was not asked where the content lives"
    );
}

/// Un `Denied` dice que clase de cosa no se pudo pedir.
///
/// El `kind` del error es parte del contrato: un cliente que ramifica por
/// el tiene que saber si lo que se le nego fue un repositorio o un asset.
#[tokio::test]
async fn a_denied_repository_is_not_reported_as_an_asset() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_with(
        db,
        Arc::new(ListedAccounts {
            allowed: Vec::new(),
        }),
    );
    store.initialize().await.expect("initialize");

    let denied = store
        .repository("acme", "releases")
        .await
        .expect_err("nobody is allowed");
    match denied {
        CatalogError::Denied { kind, .. } => assert_eq!(
            kind, "repository",
            "a denied repository was reported as another kind"
        ),
        other => panic!("unexpected error: {other:?}"),
    }

    let denied = store
        .asset("acme", "releases", "app.bin")
        .await
        .expect_err("nobody is allowed");
    match denied {
        CatalogError::Denied { kind, .. } => {
            assert_eq!(kind, "asset", "a denied asset was reported as another kind")
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

/// El nombre canónico de un repositorio tiene que ser un HRN que se pueda
/// leer.
///
/// Un repositorio cuyo nombre canónico no se puede parsear es un
/// repositorio que el resto de la plataforma no puede nombrar, y eso se
/// dice cuando se crea y no el día que una política lo pregunte.
#[tokio::test]
async fn the_canonical_name_of_a_repository_has_to_be_a_valid_hrn() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");

    let refused = store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "with a space",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect_err("the canonical name cannot be read");
    assert!(
        matches!(refused, CatalogError::InvalidRequest { .. }),
        "unexpected error: {refused:?}"
    );
}

// ---------------------------------------------------------------------------
// Lo que la segunda revisión encontró.
// ---------------------------------------------------------------------------

/// Un almacén escrito por una build más nueva no se toca.
///
/// Declarar el esquema es idempotente, pero declarar el de esta build
/// encima del de una build más nueva no deja el almacén en la versión
/// nueva: las definiciones que ya están no se rehacen. El almacén se
/// quedaría con un esquema mixto y el número de versión diría una cosa y
/// la base sería otra. La única respuesta honesta es no tocarlo y decirlo.
#[tokio::test]
async fn a_store_written_by_a_newer_schema_is_not_touched() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db.clone());
    store.initialize().await.expect("initialize");

    // Se dice que el almacén lo escribió una build más nueva.
    db.query("UPSERT $id SET applied_version = $version")
        .bind((
            "id",
            surrealdb::sql::Thing::from(("catalog_schema", "applied")),
        ))
        .bind(("version", SCHEMA_VERSION + 1))
        .await
        .expect("write the version")
        .check()
        .expect("write the version");

    let refused = store.initialize().await;
    assert!(
        matches!(
            refused,
            Err(CatalogError::SchemaMismatch {
                found,
                expected
            }) if found == SCHEMA_VERSION + 1 && expected == SCHEMA_VERSION
        ),
        "unexpected error: {refused:?}"
    );

    // Y el número del almacén sigue siendo el que tenía: esta build no
    // escribe su versión encima de la de otra.
    assert_eq!(
        store.applied_schema_version().await.expect("the version"),
        SCHEMA_VERSION + 1,
        "the version of the store was overwritten"
    );
}

/// Publicar en un repositorio que no tiene nombre no es que no haya nada.
///
/// Un repositorio vacío es una petición que no tiene sentido, no un
/// recurso que falte: la diferencia importa porque un `Missing` pide
/// reintentar y un `InvalidRequest` pide corregir.
#[tokio::test]
async fn publishing_into_an_empty_repository_is_an_invalid_request() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");

    let refused = store
        .publish(
            "acme",
            publication("", "app.bin", "pkg", "1.0", b"the content"),
        )
        .await
        .expect_err("the repository has no name");
    assert!(
        matches!(refused, CatalogError::InvalidRequest { .. }),
        "unexpected error: {refused:?}"
    );
}

/// Republicar lo mismo dice cuando se publicó, no ahora.
///
/// Una republicación idempotente no escribe nada, así que no cambia la
/// fecha del asset. Si el resultado dijera "actualizado ahora", el
/// resultado y la base dirían dos cosas distintas sobre el mismo
/// registro, y el que se fijaría del resultado creería que el contenido se
/// acaba de publicar.
#[tokio::test]
async fn an_idempotent_publication_reports_the_instant_that_is_in_the_store() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let db = open_store(&tmp.path().join("db")).await;
    let store = store_for(db);
    store.initialize().await.expect("initialize");

    store
        .create_repository(
            "acme",
            descriptor(
                "acme",
                "releases",
                RepositoryFormat::Raw,
                RepositoryMode::Hosted,
                DigestScope::Global,
            ),
        )
        .await
        .expect("create repository");

    let first = store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", b"the content"),
        )
        .await
        .expect("publish");

    let second = store
        .publish(
            "acme",
            publication("releases", "app.bin", "pkg", "1.0", b"the content"),
        )
        .await
        .expect("publish again");
    assert!(!second.created, "the second publication created something");

    let stored = store
        .asset("acme", "releases", "app.bin")
        .await
        .expect("the asset");
    assert_eq!(
        second.asset.updated_at, stored.updated_at,
        "the outcome says a different instant than the store"
    );
    assert_eq!(
        first.asset.updated_at, stored.updated_at,
        "the instant of the asset changed without writing it"
    );

    let stored_reference = store
        .reference("acme", "releases", "app.bin")
        .await
        .expect("the reference");
    assert_eq!(
        second.reference.updated_at, stored_reference.updated_at,
        "the outcome says a different instant than the store"
    );
}
