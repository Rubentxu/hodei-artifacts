//! El adaptador SurrealDB del catalogo.
//!
//! # Por que `CREATE` y no `UPSERT`
//!
//! La regla del catalogo es que una segunda escritura incompatible recibe
//! un conflicto y **no** sobrescribe. Si la escritura fuera un `UPSERT`, la
//! segunda linea ganaria por orden de llegada y el cliente que publico
//! primero veria cambiar su contenido sin que nadie se lo dijera. Por eso
//! las filas se crean con `CREATE` y el arbitro lo dan los indices
//! `UNIQUE` de la base: el indice es el unico que decide, y decide igual
//! para los dos clientes que llegan a la vez.
//!
//! # Por que una transaccion y no cuatro escrituras
//!
//! Publicar son cuatro cosas: la version, el asset, la referencia al blob
//! y las etiquetas. Publicar tres y dejar la cuarta para despues deja un
//! asset que no resuelve. Las cuatro van en una transaccion, y la
//! transaccion es una sola query con `BEGIN` y `COMMIT`: si una de las
//! cuatro falla, no queda ninguna.
//!
//! # Que motor necesita esta garantia
//!
//! La garantia de que una segunda escritura incompatible recibe un
//! conflicto se apoya en que el motor detecte el conflicto de escritura
//! sobre **el mismo registro**, y eso lo hace el motor persistente: el
//! perdedor recibe un "read or write conflict" y su transaccion no se
//! confirma. El motor en memoria no lo hace: acepta las dos escrituras
//! concurrentes del mismo registro y la ultima gana en silencio.
//!
//! Por eso los identificadores de este catalogo no son aleatorios, y por
//! eso las pruebas van contra el motor que se despliega. Un despliegue con
//! escritura concurrente no puede apoyarse en el motor en memoria.
//!
//! # Por que se relee despues de un fallo
//!
//! Cuando la transaccion se cae, el almacenamiento no dice que fallo: dice
//! que la transaccion no se ejecuto. En vez de adivinar, el catalogo vuelve
//! a leer y decide con lo que hay: si lo que hay es lo mismo que lo que
//! se iba a escribir, otro escritor publico lo mismo y esto no es un
//! conflicto; si lo que hay es distinto, es un conflicto; y si no hay
//! nada, se intenta otra vez.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_catalog::{
    AccessDecision, AccessRequest, AssetRecord, BlobIntegrityProbe, BlobProbeError, BlobReference,
    CatalogAction, CatalogAuthorizer, CatalogError, CatalogReader, CatalogWriter, DigestScope,
    MutableTag, PackageCoordinates, PackageVersion, Publication, PublishOutcome, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode,
};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::local::Db;
use surrealdb::sql::{Datetime, Thing};

/// La version del esquema del catalogo.
///
/// Cada cambio del esquema sube este numero, y `initialize` lo deja
/// escrito en la base: saber que version esta aplicada es parte de saber
/// que hay ahi.
pub const SCHEMA_VERSION: i64 = 1;

/// Cuantas veces se intenta una escritura que se ha caido por un conflicto
/// de transaccion.
///
/// Un conflicto de transaccion no es un fallo de la escritura: es la
/// escritura llegando un instante tarde.
const WRITE_ATTEMPTS: usize = 4;

/// El esquema del catalogo.
///
/// Todas las sentencias son `IF NOT EXISTS`: declarar el esquema dos veces
/// no cambia nada, y declararlo sobre una base que ya lo tiene no la
/// rompe. Eso es lo que hace que la migracion sea idempotente.
pub const SCHEMA: &str = r#"
DEFINE TABLE IF NOT EXISTS catalog_schema SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS applied_version ON catalog_schema TYPE int;
DEFINE FIELD IF NOT EXISTS applied_at ON catalog_schema TYPE datetime;

DEFINE TABLE IF NOT EXISTS catalog_repository SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS account ON catalog_repository TYPE string;
DEFINE FIELD IF NOT EXISTS repo_key ON catalog_repository TYPE string;
DEFINE FIELD IF NOT EXISTS hrn ON catalog_repository TYPE string;
DEFINE FIELD IF NOT EXISTS repo_format ON catalog_repository TYPE string;
DEFINE FIELD IF NOT EXISTS repo_mode ON catalog_repository TYPE string;
DEFINE FIELD IF NOT EXISTS config_version ON catalog_repository TYPE int;
DEFINE FIELD IF NOT EXISTS digest_scope ON catalog_repository TYPE string;
DEFINE FIELD IF NOT EXISTS config_settings ON catalog_repository TYPE object;
DEFINE FIELD IF NOT EXISTS created_at ON catalog_repository TYPE datetime;
DEFINE FIELD IF NOT EXISTS updated_at ON catalog_repository TYPE datetime;
DEFINE INDEX IF NOT EXISTS catalog_repository_key_unique ON catalog_repository FIELDS account, repo_key UNIQUE;
DEFINE INDEX IF NOT EXISTS catalog_repository_hrn_unique ON catalog_repository FIELDS hrn UNIQUE;

DEFINE TABLE IF NOT EXISTS catalog_version SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS account ON catalog_version TYPE string;
DEFINE FIELD IF NOT EXISTS repository ON catalog_version TYPE string;
DEFINE FIELD IF NOT EXISTS package_name ON catalog_version TYPE string;
DEFINE FIELD IF NOT EXISTS package_version ON catalog_version TYPE string;
DEFINE FIELD IF NOT EXISTS digest_algorithm ON catalog_version TYPE string;
DEFINE FIELD IF NOT EXISTS digest ON catalog_version TYPE string;
DEFINE FIELD IF NOT EXISTS size_bytes ON catalog_version TYPE int;
DEFINE FIELD IF NOT EXISTS published_at ON catalog_version TYPE datetime;
DEFINE INDEX IF NOT EXISTS catalog_version_unique ON catalog_version FIELDS account, repository, package_name, package_version UNIQUE;

DEFINE TABLE IF NOT EXISTS catalog_asset SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS account ON catalog_asset TYPE string;
DEFINE FIELD IF NOT EXISTS repository ON catalog_asset TYPE string;
DEFINE FIELD IF NOT EXISTS asset_path ON catalog_asset TYPE string;
DEFINE FIELD IF NOT EXISTS package_name ON catalog_asset TYPE string;
DEFINE FIELD IF NOT EXISTS package_version ON catalog_asset TYPE string;
DEFINE FIELD IF NOT EXISTS digest_algorithm ON catalog_asset TYPE string;
DEFINE FIELD IF NOT EXISTS digest ON catalog_asset TYPE string;
DEFINE FIELD IF NOT EXISTS size_bytes ON catalog_asset TYPE int;
DEFINE FIELD IF NOT EXISTS updated_at ON catalog_asset TYPE datetime;
DEFINE INDEX IF NOT EXISTS catalog_asset_unique ON catalog_asset FIELDS account, repository, asset_path UNIQUE;

DEFINE TABLE IF NOT EXISTS catalog_reference SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS account ON catalog_reference TYPE string;
DEFINE FIELD IF NOT EXISTS repository ON catalog_reference TYPE string;
DEFINE FIELD IF NOT EXISTS asset_path ON catalog_reference TYPE string;
DEFINE FIELD IF NOT EXISTS digest_algorithm ON catalog_reference TYPE string;
DEFINE FIELD IF NOT EXISTS digest ON catalog_reference TYPE string;
DEFINE FIELD IF NOT EXISTS digest_scope ON catalog_reference TYPE string;
DEFINE FIELD IF NOT EXISTS size_bytes ON catalog_reference TYPE int;
DEFINE FIELD IF NOT EXISTS updated_at ON catalog_reference TYPE datetime;
DEFINE INDEX IF NOT EXISTS catalog_reference_unique ON catalog_reference FIELDS account, repository, asset_path UNIQUE;

DEFINE TABLE IF NOT EXISTS catalog_tag SCHEMAFULL;
DEFINE FIELD IF NOT EXISTS account ON catalog_tag TYPE string;
DEFINE FIELD IF NOT EXISTS repository ON catalog_tag TYPE string;
DEFINE FIELD IF NOT EXISTS tag_name ON catalog_tag TYPE string;
DEFINE FIELD IF NOT EXISTS package_name ON catalog_tag TYPE string;
DEFINE FIELD IF NOT EXISTS package_version ON catalog_tag TYPE string;
DEFINE FIELD IF NOT EXISTS updated_at ON catalog_tag TYPE datetime;
DEFINE INDEX IF NOT EXISTS catalog_tag_unique ON catalog_tag FIELDS account, repository, tag_name UNIQUE;
"#;

/// El identificador de un registro, derivado de sus coordenadas.
///
/// El identificador NO es aleatorio, y esa es la decision que sostiene la
/// garantia de conflicto: dos clientes que publican lo mismo en el mismo
/// sitio escriben **el mismo registro**, y el motor detecta el conflicto
/// de escritura sobre el. Un identificador aleatorio haria que cada uno
/// escribiera una fila distinta, y las dos cabrian sin que ninguna se
/// enterase.
fn escape_id_part(value: &str) -> String {
    value.replace('~', "~0").replace(':', "~1")
}

/// El identificador del repositorio de una cuenta.
fn repository_id(account: &str, repo_key: &str) -> Thing {
    let id = escape_id_part(&format!("{account}|{repo_key}"));
    Thing::from(("catalog_repository", id.as_str()))
}

/// El identificador de la version publicada.
fn version_id(account: &str, repository: &str, package_name: &str, package_version: &str) -> Thing {
    let id = escape_id_part(&format!(
        "{account}|{repository}|{package_name}|{package_version}"
    ));
    Thing::from(("catalog_version", id.as_str()))
}

/// El identificador del asset de una ruta.
fn asset_id(account: &str, repository: &str, asset_path: &str) -> Thing {
    let id = escape_id_part(&format!("{account}|{repository}|{asset_path}"));
    Thing::from(("catalog_asset", id.as_str()))
}

/// El identificador de la referencia al blob de una ruta.
fn reference_id(account: &str, repository: &str, asset_path: &str) -> Thing {
    let id = escape_id_part(&format!("{account}|{repository}|{asset_path}"));
    Thing::from(("catalog_reference", id.as_str()))
}

/// El identificador de una etiqueta.
fn tag_id(account: &str, repository: &str, tag_name: &str) -> Thing {
    let id = escape_id_part(&format!("{account}|{repository}|{tag_name}"));
    Thing::from(("catalog_tag", id.as_str()))
}

/// El almacen del catalogo sobre SurrealDB.
#[derive(Clone)]
pub struct SurrealCatalogStore {
    db: Surreal<Db>,
    authorizer: Arc<dyn CatalogAuthorizer>,
    probe: Option<Arc<dyn BlobIntegrityProbe>>,
}

impl std::fmt::Debug for SurrealCatalogStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SurrealCatalogStore")
            .field("has_probe", &self.probe.is_some())
            .finish()
    }
}

impl SurrealCatalogStore {
    /// Un almacen sin decisor de permisos.
    ///
    /// Sin decisor, el catalogo no sirve nada: es el estado cerrado por
    /// defecto, no un atajo.
    pub fn new(db: Surreal<Db>) -> Self {
        Self {
            db,
            authorizer: Arc::new(hodei_catalog::DenyAllAuthorizer),
            probe: None,
        }
    }

    /// Un almacen con el decisor de permisos que se le inyecta.
    pub fn with_authorizer(db: Surreal<Db>, authorizer: Arc<dyn CatalogAuthorizer>) -> Self {
        Self {
            db,
            authorizer,
            probe: None,
        }
    }

    /// El almacen, con la sonda de integridad de los blobs.
    ///
    /// La sonda es la que permite distinguir un registro sano de uno que
    /// apunta a bytes que no son los que dice.
    pub fn with_probe(mut self, probe: Arc<dyn BlobIntegrityProbe>) -> Self {
        self.probe = Some(probe);
        self
    }

    /// Declara el esquema.
    ///
    /// Es idempotente: se puede ejecutar en caliente y en frio, y sobre una
    /// base que ya tiene el esquema no cambia nada.
    pub async fn initialize(&self) -> Result<(), CatalogError> {
        self.db
            .query(SCHEMA)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?
            .check()
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;

        self.db
            .query("UPSERT $id SET applied_version = $version, applied_at = $at")
            .bind(("id", Thing::from(("catalog_schema", "applied"))))
            .bind(("version", SCHEMA_VERSION))
            .bind(("at", Datetime::from(Utc::now())))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?
            .check()
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(())
    }

    /// La version del esquema que esta aplicada en la base.
    pub async fn applied_schema_version(&self) -> Result<i64, CatalogError> {
        #[derive(Debug, Deserialize)]
        struct Row {
            applied_version: i64,
        }

        let mut response = self
            .db
            .query("SELECT applied_version FROM catalog_schema LIMIT 1")
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<Row> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows
            .into_iter()
            .next()
            .map(|row| row.applied_version)
            .unwrap_or(0))
    }

    /// Decide si la peticion se atiende, y con que motivo no.
    async fn decide(&self, request: &AccessRequest) -> Result<(), CatalogError> {
        match self.authorizer.authorize(request).await? {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny(reason) => Err(CatalogError::Denied {
                action: request.action.as_str(),
                kind: "asset",
                key: format!("{}/{}", request.repository, request.asset_path),
                reason,
            }),
        }
    }
}

/// La fila de un repositorio, como esta en la base.
#[derive(Debug, Clone, Deserialize)]
struct RepositoryRow {
    account: String,
    repo_key: String,
    hrn: String,
    repo_format: String,
    repo_mode: String,
    config_version: i64,
    digest_scope: String,
    config_settings: serde_json::Value,
    created_at: Datetime,
    updated_at: Datetime,
}

/// La fila de una version, como esta en la base.
#[derive(Debug, Clone, Deserialize)]
struct VersionRow {
    account: String,
    repository: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    published_at: Datetime,
}

/// La fila de un asset, como esta en la base.
#[derive(Debug, Clone, Deserialize)]
struct AssetRow {
    account: String,
    repository: String,
    asset_path: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    updated_at: Datetime,
}

/// La fila de una referencia, como esta en la base.
#[derive(Debug, Clone, Deserialize)]
struct ReferenceRow {
    account: String,
    repository: String,
    asset_path: String,
    digest_algorithm: String,
    digest: String,
    digest_scope: String,
    size_bytes: i64,
    updated_at: Datetime,
}

/// La fila de una etiqueta, como esta en la base.
#[derive(Debug, Clone, Deserialize)]
struct TagRow {
    account: String,
    repository: String,
    tag_name: String,
    package_name: String,
    package_version: String,
    updated_at: Datetime,
}

/// Lo que se escribe de un repositorio.
#[derive(Debug, Clone, Serialize)]
struct RepositoryWrite {
    account: String,
    repo_key: String,
    hrn: String,
    repo_format: String,
    repo_mode: String,
    config_version: i64,
    digest_scope: String,
    config_settings: serde_json::Value,
    created_at: Datetime,
    updated_at: Datetime,
}

/// Lo que se escribe de una version.
#[derive(Debug, Clone, Serialize)]
struct VersionWrite {
    account: String,
    repository: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    published_at: Datetime,
}

/// Lo que se escribe de un asset.
#[derive(Debug, Clone, Serialize)]
struct AssetWrite {
    account: String,
    repository: String,
    asset_path: String,
    package_name: String,
    package_version: String,
    digest_algorithm: String,
    digest: String,
    size_bytes: i64,
    updated_at: Datetime,
}

/// Lo que se escribe de una referencia.
#[derive(Debug, Clone, Serialize)]
struct ReferenceWrite {
    account: String,
    repository: String,
    asset_path: String,
    digest_algorithm: String,
    digest: String,
    digest_scope: String,
    size_bytes: i64,
    updated_at: Datetime,
}

/// Lo que se escribe de una etiqueta.
#[derive(Debug, Clone, Serialize)]
struct TagWrite {
    account: String,
    repository: String,
    tag_name: String,
    package_name: String,
    package_version: String,
    updated_at: Datetime,
}

fn algorithm_of(digest: &ContentDigest) -> String {
    match digest.algorithm {
        DigestAlgorithm::Sha256 => "sha256".to_string(),
        DigestAlgorithm::Sha512 => "sha512".to_string(),
    }
}

fn digest_of(algorithm: &str, value: &str) -> Result<ContentDigest, CatalogError> {
    let algorithm = match algorithm {
        "sha256" => DigestAlgorithm::Sha256,
        "sha512" => DigestAlgorithm::Sha512,
        other => {
            return Err(CatalogError::corrupt(
                "record",
                other.to_string(),
                format!("the digest declares an algorithm this release does not know: {other}"),
            ));
        }
    };
    Ok(ContentDigest {
        algorithm,
        value: value.to_string(),
    })
}

fn size_of(size_bytes: i64) -> Result<u64, CatalogError> {
    u64::try_from(size_bytes).map_err(|_| {
        CatalogError::corrupt(
            "record",
            size_bytes.to_string(),
            "the size of the content is negative",
        )
    })
}

fn scope_of(scope: &str) -> Result<DigestScope, CatalogError> {
    match scope {
        "global" => Ok(DigestScope::Global),
        "account_partitioned" => Ok(DigestScope::AccountPartitioned),
        other => Err(CatalogError::corrupt(
            "record",
            other.to_string(),
            format!("the digest scope is not one this release knows: {other}"),
        )),
    }
}

fn format_of(format: &str) -> Result<RepositoryFormat, CatalogError> {
    match format {
        "raw" => Ok(RepositoryFormat::Raw),
        "oci" => Ok(RepositoryFormat::Oci),
        "maven2" => Ok(RepositoryFormat::Maven2),
        "npm" => Ok(RepositoryFormat::Npm),
        "pypi" => Ok(RepositoryFormat::PyPi),
        "cargo" => Ok(RepositoryFormat::Cargo),
        "nuget" => Ok(RepositoryFormat::NuGet),
        "go" => Ok(RepositoryFormat::Go),
        "helm" => Ok(RepositoryFormat::Helm),
        "apt" => Ok(RepositoryFormat::Apt),
        "rpm" => Ok(RepositoryFormat::Rpm),
        other => Err(CatalogError::corrupt(
            "repository",
            other.to_string(),
            format!("the repository declares a format this release does not know: {other}"),
        )),
    }
}

fn mode_of(mode: &str) -> Result<RepositoryMode, CatalogError> {
    match mode {
        "hosted" => Ok(RepositoryMode::Hosted),
        "proxy" => Ok(RepositoryMode::Proxy),
        "group" => Ok(RepositoryMode::Group),
        other => Err(CatalogError::corrupt(
            "repository",
            other.to_string(),
            format!("the repository declares a mode this release does not know: {other}"),
        )),
    }
}

impl RepositoryRow {
    fn into_descriptor(self) -> Result<RepositoryDescriptor, CatalogError> {
        let hrn = Hrn::from_string(&self.hrn).ok_or_else(|| {
            CatalogError::corrupt(
                "repository",
                self.hrn.clone(),
                "the canonical name of the repository is not a HRN this release can read",
            )
        })?;
        Ok(RepositoryDescriptor {
            hrn,
            account: self.account,
            repo_key: self.repo_key,
            format: format_of(&self.repo_format)?,
            mode: mode_of(&self.repo_mode)?,
            config: RepositoryConfig {
                version: u64::try_from(self.config_version).unwrap_or(0),
                digest_scope: scope_of(&self.digest_scope)?,
                config_settings: self.config_settings,
            },
            created_at: DateTime::<Utc>::from(self.created_at),
            updated_at: DateTime::<Utc>::from(self.updated_at),
        })
    }
}

impl VersionRow {
    fn into_version(self) -> Result<PackageVersion, CatalogError> {
        Ok(PackageVersion {
            account: self.account,
            repository: self.repository,
            coordinates: PackageCoordinates {
                package_name: self.package_name,
                package_version: self.package_version,
            },
            digest: digest_of(&self.digest_algorithm, &self.digest)?,
            size_bytes: size_of(self.size_bytes)?,
            published_at: DateTime::<Utc>::from(self.published_at),
        })
    }
}

impl AssetRow {
    fn into_asset(self) -> Result<AssetRecord, CatalogError> {
        Ok(AssetRecord {
            account: self.account,
            repository: self.repository,
            asset_path: self.asset_path,
            coordinates: PackageCoordinates {
                package_name: self.package_name,
                package_version: self.package_version,
            },
            digest: digest_of(&self.digest_algorithm, &self.digest)?,
            size_bytes: size_of(self.size_bytes)?,
            updated_at: DateTime::<Utc>::from(self.updated_at),
        })
    }
}

impl ReferenceRow {
    fn into_reference(self) -> Result<BlobReference, CatalogError> {
        Ok(BlobReference {
            account: self.account,
            repository: self.repository,
            asset_path: self.asset_path,
            digest: digest_of(&self.digest_algorithm, &self.digest)?,
            scope: scope_of(&self.digest_scope)?,
            size_bytes: size_of(self.size_bytes)?,
            updated_at: DateTime::<Utc>::from(self.updated_at),
        })
    }
}

impl TagRow {
    fn into_tag(self) -> Result<MutableTag, CatalogError> {
        Ok(MutableTag {
            account: self.account,
            repository: self.repository,
            tag_name: self.tag_name,
            coordinates: PackageCoordinates {
                package_name: self.package_name,
                package_version: self.package_version,
            },
            updated_at: DateTime::<Utc>::from(self.updated_at),
        })
    }
}

/// Lo que hay que escribir, y lo que ya hay.
///
/// El plan se calcula antes de escribir: se lee lo que hay, se decide que
/// filas faltan, y se escriben solo esas. Las que ya estan con el mismo
/// contenido no se tocan.
#[derive(Debug)]
struct PublicationPlan {
    version: PackageVersion,
    asset: AssetRecord,
    reference: BlobReference,
    create_version: bool,
    create_asset: bool,
    create_reference: bool,
    create_tags: Vec<String>,
    /// El conflicto que impide la publicacion, si lo hay.
    conflict: Option<CatalogError>,
}

impl PublicationPlan {
    /// Si no hay nada que escribir.
    fn is_complete(&self) -> bool {
        !self.create_version
            && !self.create_asset
            && !self.create_reference
            && self.create_tags.is_empty()
    }
}

/// El fallo de una escritura.
///
/// La transaccion no se ha confirmado. El almacenamiento no dice por que,
/// y no hace falta: lo que decide es lo que hay en la base despues de
/// releer, no la causa del fallo.
#[derive(Debug)]
struct WriteFailure {
    /// Lo que dijo el almacenamiento.
    detail: String,
}

#[async_trait]
impl CatalogReader for SurrealCatalogStore {
    async fn repository(
        &self,
        principal_account: &str,
        repo_key: &str,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        self.decide(&AccessRequest {
            principal_account: principal_account.to_string(),
            action: CatalogAction::Read,
            repository: repo_key.to_string(),
            asset_path: String::new(),
        })
        .await?;

        let mut response = self
            .db
            .query(
                "SELECT account, repo_key, hrn, repo_format, repo_mode, config_version, \
                 digest_scope, config_settings, created_at, updated_at FROM catalog_repository \
                 WHERE account = $account AND repo_key = $repo_key LIMIT 1",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("repo_key", repo_key.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<RepositoryRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        rows.into_iter()
            .next()
            .ok_or_else(|| CatalogError::missing("repository", repo_key.to_string()))?
            .into_descriptor()
    }

    async fn asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<AssetRecord, CatalogError> {
        self.decide(&AccessRequest::read(
            principal_account,
            repository,
            asset_path,
        ))
        .await?;

        let key = format!("{repository}/{asset_path}");
        let asset = self
            .the_asset_row(principal_account, repository, asset_path)
            .await?;
        let Some(asset) = asset else {
            return Err(CatalogError::missing("asset", key));
        };
        let asset = asset.into_asset()?;

        // El asset tiene que apuntar a una version que exista y que diga
        // lo mismo que el asset. Si no, el registro esta roto.
        let version = self
            .the_version_row(
                principal_account,
                repository,
                &asset.coordinates.package_name,
                &asset.coordinates.package_version,
            )
            .await?;
        let Some(version) = version else {
            return Err(CatalogError::corrupt(
                "asset",
                key,
                "the asset points at a version that is not in the catalog",
            ));
        };
        let version = version.into_version()?;
        if version.digest != asset.digest {
            return Err(CatalogError::corrupt(
                "asset",
                key,
                "the asset and the version it points at have different digests",
            ));
        }
        if !asset.digest.is_well_formed() {
            return Err(CatalogError::corrupt(
                "asset",
                key,
                "the digest of the asset is not well formed",
            ));
        }

        // Y la referencia al blob tiene que decir lo mismo.
        let reference = self
            .the_reference_row(principal_account, repository, asset_path)
            .await?;
        let Some(reference) = reference else {
            return Err(CatalogError::corrupt(
                "asset",
                key,
                "the asset has no reference to a blob",
            ));
        };
        let reference = reference.into_reference()?;
        if reference.digest != asset.digest {
            return Err(CatalogError::corrupt(
                "asset",
                key,
                "the asset and its blob reference have different digests",
            ));
        }

        // Y los bytes tienen que ser los que el catalogo dice.
        if let Some(probe) = &self.probe {
            match probe.length_of(&asset.digest).await {
                Ok(size) if size != asset.size_bytes => {
                    return Err(CatalogError::corrupt(
                        "asset",
                        key,
                        format!(
                            "the blob has {} bytes and the catalog says {}",
                            size, asset.size_bytes
                        ),
                    ));
                }
                Err(BlobProbeError::Missing) => {
                    return Err(CatalogError::corrupt(
                        "asset",
                        key,
                        "the blob the catalog points at is not in the store",
                    ));
                }
                Err(BlobProbeError::Unavailable { detail }) => {
                    return Err(CatalogError::unavailable(detail));
                }
                Ok(_) => {}
            }
        }

        Ok(asset)
    }

    async fn version(
        &self,
        principal_account: &str,
        repository: &str,
        coordinates: &PackageCoordinates,
    ) -> Result<PackageVersion, CatalogError> {
        self.decide(&AccessRequest {
            principal_account: principal_account.to_string(),
            action: CatalogAction::Read,
            repository: repository.to_string(),
            asset_path: coordinates.package_name.clone(),
        })
        .await?;

        let key = format!(
            "{repository}/{}@{}",
            coordinates.package_name, coordinates.package_version
        );
        let row = self
            .the_version_row(
                principal_account,
                repository,
                &coordinates.package_name,
                &coordinates.package_version,
            )
            .await?;
        row.ok_or_else(|| CatalogError::missing("version", key))?
            .into_version()
    }

    async fn tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
    ) -> Result<MutableTag, CatalogError> {
        self.decide(&AccessRequest::read(
            principal_account,
            repository,
            tag_name,
        ))
        .await?;

        let mut response = self
            .db
            .query(
                "SELECT account, repository, tag_name, package_name, package_version, updated_at \
                 FROM catalog_tag WHERE account = $account AND repository = $repository \
                 AND tag_name = $tag_name LIMIT 1",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("tag_name", tag_name.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<TagRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        rows.into_iter()
            .next()
            .ok_or_else(|| CatalogError::missing("tag", tag_name.to_string()))?
            .into_tag()
    }

    async fn reference(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<BlobReference, CatalogError> {
        self.decide(&AccessRequest::read(
            principal_account,
            repository,
            asset_path,
        ))
        .await?;

        let row = self
            .the_reference_row(principal_account, repository, asset_path)
            .await?;
        row.ok_or_else(|| CatalogError::missing("reference", format!("{repository}/{asset_path}")))?
            .into_reference()
    }
}

impl SurrealCatalogStore {
    /// La fila del asset, acotada por la cuenta.
    async fn the_asset_row(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<Option<AssetRow>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT account, repository, asset_path, package_name, package_version, \
                 digest_algorithm, digest, size_bytes, updated_at FROM catalog_asset \
                 WHERE account = $account AND repository = $repository AND asset_path = $asset_path \
                 LIMIT 1",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("asset_path", asset_path.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<AssetRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows.into_iter().next())
    }

    /// La fila de la version, acotada por la cuenta.
    async fn the_version_row(
        &self,
        principal_account: &str,
        repository: &str,
        package_name: &str,
        package_version: &str,
    ) -> Result<Option<VersionRow>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT account, repository, package_name, package_version, digest_algorithm, \
                 digest, size_bytes, published_at FROM catalog_version \
                 WHERE account = $account AND repository = $repository \
                 AND package_name = $package_name AND package_version = $package_version LIMIT 1",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("package_name", package_name.to_string()))
            .bind(("package_version", package_version.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<VersionRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows.into_iter().next())
    }

    /// La fila de la referencia, acotada por la cuenta.
    async fn the_reference_row(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<Option<ReferenceRow>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT account, repository, asset_path, digest_algorithm, digest, digest_scope, \
                 size_bytes, updated_at FROM catalog_reference \
                 WHERE account = $account AND repository = $repository AND asset_path = $asset_path \
                 LIMIT 1",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("asset_path", asset_path.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<ReferenceRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows.into_iter().next())
    }

    /// La fila de la etiqueta, acotada por la cuenta.
    async fn the_tag_row(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
    ) -> Result<Option<TagRow>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT account, repository, tag_name, package_name, package_version, updated_at \
                 FROM catalog_tag WHERE account = $account AND repository = $repository \
                 AND tag_name = $tag_name LIMIT 1",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("repository", repository.to_string()))
            .bind(("tag_name", tag_name.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<TagRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows.into_iter().next())
    }

    /// El repositorio de la cuenta, sin fila si no lo hay.
    async fn the_repository_row(
        &self,
        principal_account: &str,
        repo_key: &str,
    ) -> Result<Option<RepositoryRow>, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT account, repo_key, hrn, repo_format, repo_mode, config_version, \
                 digest_scope, config_settings, created_at, updated_at FROM catalog_repository \
                 WHERE account = $account AND repo_key = $repo_key LIMIT 1",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("repo_key", repo_key.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<RepositoryRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows.into_iter().next())
    }
}

#[async_trait]
impl CatalogWriter for SurrealCatalogStore {
    async fn create_repository(
        &self,
        principal_account: &str,
        descriptor: RepositoryDescriptor,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        self.decide(&AccessRequest {
            principal_account: principal_account.to_string(),
            action: CatalogAction::Configure,
            repository: descriptor.repo_key.clone(),
            asset_path: String::new(),
        })
        .await?;

        // Un modo que esta release no sirve se dice que no, y no se crea un
        // repositorio que luego no va a funcionar.
        if !descriptor.mode.is_served_by_this_release() {
            return Err(CatalogError::ModeNotImplemented {
                mode: descriptor.mode,
            });
        }
        if !descriptor.config.has_readable_settings() {
            return Err(CatalogError::invalid(
                "the settings of the repository are not an object",
            ));
        }
        if descriptor.account != principal_account {
            return Err(CatalogError::Denied {
                action: "configure",
                kind: "repository",
                key: descriptor.repo_key.clone(),
                reason: "the repository belongs to another account".to_string(),
            });
        }
        if descriptor.hrn.account_id() != descriptor.account {
            return Err(CatalogError::invalid(
                "the canonical name of the repository names another account",
            ));
        }
        if descriptor.repo_key.trim().is_empty() {
            return Err(CatalogError::invalid("the key of the repository is empty"));
        }

        let now = Utc::now();
        let row = RepositoryWrite {
            account: descriptor.account.clone(),
            repo_key: descriptor.repo_key.clone(),
            hrn: descriptor.hrn.to_string(),
            repo_format: descriptor.format.as_str().to_string(),
            repo_mode: descriptor.mode.as_str().to_string(),
            config_version: i64::try_from(descriptor.config.version).unwrap_or(1),
            digest_scope: scope_name(descriptor.config.digest_scope),
            config_settings: descriptor.config.config_settings.clone(),
            created_at: Datetime::from(now),
            updated_at: Datetime::from(now),
        };

        self.db
            .query("CREATE $id CONTENT $row")
            .bind((
                "id",
                repository_id(&descriptor.account, &descriptor.repo_key),
            ))
            .bind(("row", row))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?
            .check()
            .map_err(|error| {
                let message = error.to_string();
                if message.contains("already contains") || message.contains("already exists") {
                    CatalogError::conflict(
                        "repository",
                        descriptor.repo_key.clone(),
                        "there is already a repository with that key or that canonical name",
                    )
                } else {
                    CatalogError::unavailable(message)
                }
            })?;

        self.repository(principal_account, &descriptor.repo_key)
            .await
    }

    async fn update_config(
        &self,
        principal_account: &str,
        repo_key: &str,
        config: RepositoryConfig,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        self.decide(&AccessRequest {
            principal_account: principal_account.to_string(),
            action: CatalogAction::Configure,
            repository: repo_key.to_string(),
            asset_path: String::new(),
        })
        .await?;

        if !config.has_readable_settings() {
            return Err(CatalogError::invalid(
                "the settings of the repository are not an object",
            ));
        }

        let current = self
            .the_repository_row(principal_account, repo_key)
            .await?
            .ok_or_else(|| CatalogError::missing("repository", repo_key.to_string()))?;
        let current = current.into_descriptor()?;

        // La version de la configuracion sube siempre: dos configuraciones
        // con la misma version tienen que ser la misma configuracion.
        let version = i64::try_from(current.config.version.saturating_add(1)).unwrap_or(i64::MAX);

        let mut response = self
            .db
            .query(
                "UPDATE $id SET config_version = $version, digest_scope = $scope, \
                 config_settings = $settings, updated_at = $at \
                 RETURN account, repo_key, hrn, repo_format, repo_mode, config_version, \
                 digest_scope, config_settings, created_at, updated_at",
            )
            .bind(("id", repository_id(principal_account, repo_key)))
            .bind(("version", version))
            .bind(("scope", scope_name(config.digest_scope)))
            .bind(("settings", config.config_settings.clone()))
            .bind(("at", Datetime::from(Utc::now())))
            .bind(("account", principal_account.to_string()))
            .bind(("repo_key", repo_key.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<RepositoryRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        rows.into_iter()
            .next()
            .ok_or_else(|| CatalogError::missing("repository", repo_key.to_string()))?
            .into_descriptor()
    }

    async fn publish(
        &self,
        principal_account: &str,
        publication: Publication,
    ) -> Result<PublishOutcome, CatalogError> {
        if publication.asset_path.trim().is_empty() {
            return Err(CatalogError::invalid("the path of the asset is empty"));
        }
        if publication.coordinates.package_name.trim().is_empty() {
            return Err(CatalogError::invalid("the name of the package is empty"));
        }
        if publication.coordinates.package_version.trim().is_empty() {
            return Err(CatalogError::invalid("the version of the package is empty"));
        }
        if !publication.digest.is_well_formed() {
            return Err(CatalogError::invalid("the digest is not well formed"));
        }

        self.decide(&AccessRequest::publish(
            principal_account,
            &publication.repository,
            &publication.asset_path,
        ))
        .await?;

        let repository = self
            .the_repository_row(principal_account, &publication.repository)
            .await?
            .ok_or_else(|| CatalogError::missing("repository", publication.repository.clone()))?
            .into_descriptor()?;

        // Solo se publica en un repositorio que esta release sirva.
        if !repository.mode.is_served_by_this_release() {
            return Err(CatalogError::ModeNotImplemented {
                mode: repository.mode,
            });
        }

        let scope = repository.config.digest_scope;

        let mut last_failure = String::new();
        for _attempt in 0..WRITE_ATTEMPTS {
            let plan = self
                .plan_the_publication(principal_account, &publication, scope)
                .await?;
            if let Some(conflict) = plan.conflict {
                return Err(conflict);
            }
            if plan.is_complete() {
                return Ok(the_outcome(&plan, false));
            }

            match self.write_the_publication(&plan).await {
                Ok(()) => return Ok(the_outcome(&plan, true)),
                Err(failure) => {
                    // Puede que otro escritor publicara lo mismo a la vez.
                    // El siguiente intento vuelve a leer y decide con lo
                    // que hay, no con la causa del fallo.
                    last_failure = failure.detail;
                }
            }
        }

        // El ultimo intento tambien se relee: perder la carrera en el
        // ultimo intento es un conflicto, no un fallo del almacenamiento.
        let plan = self
            .plan_the_publication(principal_account, &publication, scope)
            .await?;
        if let Some(conflict) = plan.conflict {
            return Err(conflict);
        }
        if plan.is_complete() {
            return Ok(the_outcome(&plan, false));
        }

        Err(CatalogError::unavailable(last_failure))
    }

    async fn move_tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
        coordinates: PackageCoordinates,
    ) -> Result<MutableTag, CatalogError> {
        self.decide(&AccessRequest {
            principal_account: principal_account.to_string(),
            action: CatalogAction::MoveTag,
            repository: repository.to_string(),
            asset_path: tag_name.to_string(),
        })
        .await?;

        if tag_name.trim().is_empty() {
            return Err(CatalogError::invalid("the name of the tag is empty"));
        }

        let descriptor = self
            .the_repository_row(principal_account, repository)
            .await?
            .ok_or_else(|| CatalogError::missing("repository", repository.to_string()))?
            .into_descriptor()?;
        if !descriptor.mode.is_served_by_this_release() {
            return Err(CatalogError::ModeNotImplemented {
                mode: descriptor.mode,
            });
        }

        // La etiqueta tiene que apuntar a una version que exista. Las
        // versiones no se borran, asi que esta comprobacion no se queda
        // vieja entre la lectura y la escritura.
        let version = self
            .the_version_row(
                principal_account,
                repository,
                &coordinates.package_name,
                &coordinates.package_version,
            )
            .await?
            .ok_or_else(|| {
                CatalogError::missing(
                    "version",
                    format!(
                        "{repository}/{}@{}",
                        coordinates.package_name, coordinates.package_version
                    ),
                )
            })?;

        // Mover la etiqueta escribe la etiqueta y nada mas: ni la version
        // ni el blob que hay detrás cambian.
        self.db
            .query("UPSERT $id CONTENT $tag")
            .bind(("id", tag_id(principal_account, repository, tag_name)))
            .bind((
                "tag",
                TagWrite {
                    account: principal_account.to_string(),
                    repository: repository.to_string(),
                    tag_name: tag_name.to_string(),
                    package_name: coordinates.package_name.clone(),
                    package_version: coordinates.package_version.clone(),
                    updated_at: Datetime::from(Utc::now()),
                },
            ))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?
            .check()
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;

        // La version se lee para dejar constancia de que existe, y para
        // que un tag no pueda apuntar a algo que no esta.
        let _ = version;
        self.tag(principal_account, repository, tag_name).await
    }

    async fn move_asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
        coordinates: PackageCoordinates,
    ) -> Result<AssetRecord, CatalogError> {
        self.decide(&AccessRequest {
            principal_account: principal_account.to_string(),
            action: CatalogAction::MoveAsset,
            repository: repository.to_string(),
            asset_path: asset_path.to_string(),
        })
        .await?;

        let descriptor = self
            .the_repository_row(principal_account, repository)
            .await?
            .ok_or_else(|| CatalogError::missing("repository", repository.to_string()))?
            .into_descriptor()?;
        if !descriptor.mode.is_served_by_this_release() {
            return Err(CatalogError::ModeNotImplemented {
                mode: descriptor.mode,
            });
        }

        // El asset tiene que existir: mover lo que no esta no es mover,
        // es inventar.
        let asset = self
            .the_asset_row(principal_account, repository, asset_path)
            .await?
            .ok_or_else(|| CatalogError::missing("asset", format!("{repository}/{asset_path}")))?
            .into_asset()?;

        // Y la referencia tambien: un asset sin referencia esta roto, y
        // mover un asset roto no lo arregla.
        let reference = self
            .the_reference_row(principal_account, repository, asset_path)
            .await?
            .ok_or_else(|| {
                CatalogError::corrupt(
                    "asset",
                    format!("{repository}/{asset_path}"),
                    "the asset has no reference to a blob",
                )
            })?
            .into_reference()?;

        // La version de destino tiene que existir. Las versiones no se
        // borran, asi que esta comprobacion no se queda vieja.
        let version = self
            .the_version_row(
                principal_account,
                repository,
                &coordinates.package_name,
                &coordinates.package_version,
            )
            .await?
            .ok_or_else(|| {
                CatalogError::missing(
                    "version",
                    format!(
                        "{repository}/{}@{}",
                        coordinates.package_name, coordinates.package_version
                    ),
                )
            })?
            .into_version()?;

        let scope = descriptor.config.digest_scope;
        let now = Datetime::from(Utc::now());

        // El asset y su referencia se mueven juntos: si solo se moviera el
        // asset, la descarga serviria un blob y la recoleccion guardaria
        // otro.
        let sql = "BEGIN TRANSACTION;\n\
                   UPDATE $asset_id SET package_name = $package_name, \
                   package_version = $package_version, digest_algorithm = $digest_algorithm, \
                   digest = $digest, size_bytes = $size_bytes, updated_at = $at;\n\
                   UPDATE $reference_id SET digest_algorithm = $digest_algorithm, \
                   digest = $digest, digest_scope = $digest_scope, size_bytes = $size_bytes, \
                   updated_at = $at;\n\
                   COMMIT;\n";

        let response = self
            .db
            .query(sql)
            .bind(("package_name", coordinates.package_name.clone()))
            .bind(("package_version", coordinates.package_version.clone()))
            .bind(("digest_algorithm", algorithm_of(&version.digest)))
            .bind(("digest", version.digest.value.clone()))
            .bind(("size_bytes", i64::try_from(version.size_bytes).unwrap_or(0)))
            .bind(("digest_scope", scope_name(scope)))
            .bind(("at", now))
            .bind((
                "asset_id",
                asset_id(principal_account, repository, asset_path),
            ))
            .bind((
                "reference_id",
                reference_id(principal_account, repository, asset_path),
            ))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        response
            .check()
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;

        // El asset que ha quedado es el que se pidio; y la referencia
        // tiene que decir lo mismo que el.
        let moved = self
            .asset(principal_account, repository, asset_path)
            .await?;
        if moved.digest != version.digest {
            return Err(CatalogError::corrupt(
                "asset",
                format!("{repository}/{asset_path}"),
                "the asset does not point at the version it was moved to",
            ));
        }
        let moved_reference = self
            .reference(principal_account, repository, asset_path)
            .await?;
        if moved_reference.digest != moved.digest {
            return Err(CatalogError::corrupt(
                "asset",
                format!("{repository}/{asset_path}"),
                "the asset and its blob reference point at different content",
            ));
        }

        // Y la version que se queda atras conserva su digest y sus bytes.
        let _ = (asset, reference);
        Ok(moved)
    }
}

impl SurrealCatalogStore {
    /// Decide que hay que escribir para esta publicacion.
    ///
    /// Se lee lo que hay y se compara con lo que se quiere publicar. Lo que
    /// esta con el mismo contenido se reutiliza; lo que esta con otro
    /// contenido es un conflicto, y no se escribe.
    async fn plan_the_publication(
        &self,
        principal_account: &str,
        publication: &Publication,
        scope: DigestScope,
    ) -> Result<PublicationPlan, CatalogError> {
        let now = Utc::now();
        let account = principal_account.to_string();
        let repository = publication.repository.clone();
        let coordinates = publication.coordinates.clone();
        let digest = publication.digest.clone();
        let size_bytes = i64::try_from(publication.size_bytes).map_err(|_| {
            CatalogError::invalid("the size of the content does not fit in the catalog")
        })?;

        // La version.
        let existing_version = self
            .the_version_row(
                principal_account,
                &repository,
                &coordinates.package_name,
                &coordinates.package_version,
            )
            .await?;
        let version = PackageVersion {
            account: account.clone(),
            repository: repository.clone(),
            coordinates: coordinates.clone(),
            digest: digest.clone(),
            size_bytes: publication.size_bytes,
            published_at: existing_version
                .as_ref()
                .map(|row| DateTime::<Utc>::from(row.published_at.clone()))
                .unwrap_or(now),
        };
        let mut conflict = None;
        let mut create_version = true;
        if let Some(row) = existing_version {
            let same = row.digest == digest.value
                && row.digest_algorithm == algorithm_of(&digest)
                && row.size_bytes == size_bytes;
            if same {
                create_version = false;
            } else {
                conflict = Some(CatalogError::conflict(
                    "version",
                    format!(
                        "{repository}/{}@{}",
                        coordinates.package_name, coordinates.package_version
                    ),
                    "there is already a version published with a different content",
                ));
            }
        }

        // El asset.
        let existing_asset = self
            .the_asset_row(principal_account, &repository, &publication.asset_path)
            .await?;
        let asset = AssetRecord {
            account: account.clone(),
            repository: repository.clone(),
            asset_path: publication.asset_path.clone(),
            coordinates: coordinates.clone(),
            digest: digest.clone(),
            size_bytes: publication.size_bytes,
            updated_at: now,
        };
        let mut create_asset = true;
        if let Some(row) = existing_asset {
            let same = row.package_name == coordinates.package_name
                && row.package_version == coordinates.package_version
                && row.digest == digest.value
                && row.digest_algorithm == algorithm_of(&digest)
                && row.size_bytes == size_bytes;
            if same {
                create_asset = false;
            } else if conflict.is_none() {
                conflict = Some(CatalogError::conflict(
                    "asset",
                    format!("{repository}/{}", publication.asset_path),
                    "the path already points at another version",
                ));
            }
        }

        // La referencia al blob.
        let existing_reference = self
            .the_reference_row(principal_account, &repository, &publication.asset_path)
            .await?;
        let reference = BlobReference {
            account: account.clone(),
            repository: repository.clone(),
            asset_path: publication.asset_path.clone(),
            digest: digest.clone(),
            scope,
            size_bytes: publication.size_bytes,
            updated_at: now,
        };
        let mut create_reference = true;
        if let Some(row) = existing_reference {
            let same = row.digest == digest.value
                && row.digest_algorithm == algorithm_of(&digest)
                && row.digest_scope == scope_name(scope)
                && row.size_bytes == size_bytes;
            if same {
                create_reference = false;
            } else if conflict.is_none() {
                conflict = Some(CatalogError::conflict(
                    "reference",
                    format!("{repository}/{}", publication.asset_path),
                    "the path already references another blob",
                ));
            }
        }

        // Las etiquetas.
        let mut create_tags = Vec::new();
        for tag_name in &publication.tags {
            if tag_name.trim().is_empty() {
                return Err(CatalogError::invalid("a tag name is empty"));
            }
            let existing = self
                .the_tag_row(principal_account, &repository, tag_name)
                .await?;
            match existing {
                None => create_tags.push(tag_name.clone()),
                Some(row)
                    if row.package_name == coordinates.package_name
                        && row.package_version == coordinates.package_version => {}
                Some(_) => {
                    if conflict.is_none() {
                        conflict = Some(CatalogError::conflict(
                            "tag",
                            format!("{repository}/{tag_name}"),
                            "the tag already points at another version",
                        ));
                    }
                }
            }
        }

        Ok(PublicationPlan {
            version,
            asset,
            reference,
            create_version,
            create_asset,
            create_reference,
            create_tags,
            conflict,
        })
    }

    /// Escribe lo que falta, en una sola transaccion.
    async fn write_the_publication(&self, plan: &PublicationPlan) -> Result<(), WriteFailure> {
        let mut sql = String::from("BEGIN TRANSACTION;\n");
        let mut version = None;
        let mut version_record_id = None;
        let mut asset = None;
        let mut asset_record_id = None;
        let mut reference = None;
        let mut reference_record_id = None;
        let mut tags = Vec::new();

        if plan.create_version {
            sql.push_str("CREATE $version_id CONTENT $version;\n");
            version_record_id = Some(version_id(
                &plan.version.account,
                &plan.version.repository,
                &plan.version.coordinates.package_name,
                &plan.version.coordinates.package_version,
            ));
            version = Some(VersionWrite {
                account: plan.version.account.clone(),
                repository: plan.version.repository.clone(),
                package_name: plan.version.coordinates.package_name.clone(),
                package_version: plan.version.coordinates.package_version.clone(),
                digest_algorithm: algorithm_of(&plan.version.digest),
                digest: plan.version.digest.value.clone(),
                size_bytes: i64::try_from(plan.version.size_bytes).unwrap_or(0),
                published_at: Datetime::from(plan.version.published_at),
            });
        }
        if plan.create_asset {
            sql.push_str("CREATE $asset_id CONTENT $asset;\n");
            asset_record_id = Some(asset_id(
                &plan.asset.account,
                &plan.asset.repository,
                &plan.asset.asset_path,
            ));
            asset = Some(AssetWrite {
                account: plan.asset.account.clone(),
                repository: plan.asset.repository.clone(),
                asset_path: plan.asset.asset_path.clone(),
                package_name: plan.asset.coordinates.package_name.clone(),
                package_version: plan.asset.coordinates.package_version.clone(),
                digest_algorithm: algorithm_of(&plan.asset.digest),
                digest: plan.asset.digest.value.clone(),
                size_bytes: i64::try_from(plan.asset.size_bytes).unwrap_or(0),
                updated_at: Datetime::from(plan.asset.updated_at),
            });
        }
        if plan.create_reference {
            sql.push_str("CREATE $reference_id CONTENT $reference;\n");
            reference_record_id = Some(reference_id(
                &plan.reference.account,
                &plan.reference.repository,
                &plan.reference.asset_path,
            ));
            reference = Some(ReferenceWrite {
                account: plan.reference.account.clone(),
                repository: plan.reference.repository.clone(),
                asset_path: plan.reference.asset_path.clone(),
                digest_algorithm: algorithm_of(&plan.reference.digest),
                digest: plan.reference.digest.value.clone(),
                digest_scope: scope_name(plan.reference.scope),
                size_bytes: i64::try_from(plan.reference.size_bytes).unwrap_or(0),
                updated_at: Datetime::from(plan.reference.updated_at),
            });
        }
        for (index, tag_name) in plan.create_tags.iter().enumerate() {
            sql.push_str(&format!("CREATE $tag{index}_id CONTENT $tag{index};\n"));
            tags.push((
                format!("tag{index}"),
                tag_id(&plan.asset.account, &plan.asset.repository, tag_name),
                TagWrite {
                    account: plan.asset.account.clone(),
                    repository: plan.asset.repository.clone(),
                    tag_name: tag_name.clone(),
                    package_name: plan.asset.coordinates.package_name.clone(),
                    package_version: plan.asset.coordinates.package_version.clone(),
                    updated_at: Datetime::from(plan.asset.updated_at),
                },
            ));
        }
        sql.push_str("COMMIT;\n");

        let mut statement = self.db.query(sql);
        if let (Some(id), Some(version)) = (version_record_id, version) {
            statement = statement
                .bind(("version_id", id))
                .bind(("version", version));
        }
        if let (Some(id), Some(asset)) = (asset_record_id, asset) {
            statement = statement.bind(("asset_id", id)).bind(("asset", asset));
        }
        if let (Some(id), Some(reference)) = (reference_record_id, reference) {
            statement = statement
                .bind(("reference_id", id))
                .bind(("reference", reference));
        }
        for (name, id, tag) in tags {
            statement = statement.bind((format!("{name}_id"), id)).bind((name, tag));
        }

        match statement.await {
            Err(error) => Err(WriteFailure {
                detail: error.to_string(),
            }),
            Ok(response) => match response.check() {
                Ok(_) => Ok(()),
                Err(error) => Err(WriteFailure {
                    detail: error.to_string(),
                }),
            },
        }
    }
}

/// El nombre con el que se guarda un ambito de digest.
fn scope_name(scope: DigestScope) -> String {
    match scope {
        DigestScope::Global => "global".to_string(),
        DigestScope::AccountPartitioned => "account_partitioned".to_string(),
    }
}

/// Lo que la publicacion deja, con lo que se ha creado.
fn the_outcome(plan: &PublicationPlan, created: bool) -> PublishOutcome {
    PublishOutcome {
        version: plan.version.clone(),
        asset: plan.asset.clone(),
        reference: plan.reference.clone(),
        created,
    }
}
