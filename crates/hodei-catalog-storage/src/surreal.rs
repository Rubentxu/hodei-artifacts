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
//! Que "si una de las cuatro falla, no queda ninguna" no es una promesa
//! del catalogo sino del motor, y por eso se comprobo en el motor que se
//! usa. En SurrealDB 2.7 el nucleo ejecuta la transaccion con control
//! optimista y, cuando una sentencia del lote falla, **cancela** la
//! transaccion y no ejecuta el `COMMIT` que venia detras
//! (`surrealdb-core-2.7.0/src/dbs/executor.rs`); y el `check()` de la
//! respuesta recorre **todos** los resultados del lote, no solo el ultimo
//! (`surrealdb-2.7.0/src/api/method/query.rs`), asi que el fallo de la
//! tercera sentencia no se esconde detras del `COMMIT`. El adaptador se
//! apoya en las dos cosas, y por eso fija la version 2.7 como suelo.
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
//! Y una frontera que conviene no disimular: el almacen **no puede
//! distinguir** los motores embebidos en tiempo de ejecucion. `Mem` y
//! `RocksDB` se abren los dos como `Surreal<Db>`, y la API no expone que
//! motor hay debajo. Por eso la garantia no se comprueba con un `if`, sino
//! con una convencion asumida: quien despliegue esto abre el motor
//! persistente, y las pruebas de este crate se hacen contra ese motor
//! precisamente para que la convencion no se pueda incumplir sin que se
//! vea.
//!
//! # Por que se relee despues de un fallo
//!
//! Cuando la transaccion se cae, el almacenamiento no dice que fallo: dice
//! que la transaccion no se ejecuto. En vez de adivinar, el catalogo vuelve
//! a leer y decide con lo que hay: si lo que hay es lo mismo que lo que
//! se iba a escribir, otro escritor publico lo mismo y esto no es un
//! conflicto; si lo que hay es distinto, es un conflicto; y si no hay
//! nada, se intenta otra vez.
//!
//! # Donde acaba el aislamiento de cuentas
//!
//! El catalogo no verifica **quien** es el principal: recibe una cuenta ya
//! verificada por la frontera, que es quien autentico y quien decidio.
//! Delegar eso es la decision que hace que el catalogo sea un contexto
//! acotado y no un sistema de identidades.
//!
//! Lo que el catalogo si garantiza es no traicionar esa frontera: la
//! cuenta va en el filtro de cada consulta y en el identificador de cada
//! registro, la decision se pide **antes** de tocar la base, y un `Denied`
//! no dice si lo que se pidio existia. El que le pasa una cuenta que no
//! es la suya no encuentra un fallo en el catalogo: encuentra que el
//! catalogo no le cree, y para eso esta el puerto de autorizacion.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_catalog::{
    AccessDecision, AccessRequest, AssetRecord, BlobIntegrityProbe, BlobLocation, BlobProbeError,
    BlobReference, CatalogAction, CatalogAuthorizer, CatalogError, CatalogReader, CatalogResource,
    CatalogWriter, DigestScope, MutableTag, PackageCoordinates, PackageVersion, Publication,
    PublishOutcome, RepositoryConfig, RepositoryDescriptor, RepositoryFormat, RepositoryMode,
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
///
/// Para que eso sea cierto, la codificacion tiene que ser **inyectiva**:
/// dos coordenadas distintas no pueden producir el mismo registro. Por eso
/// cada parte se escapa por separado y despues se unen con un separador
/// que ninguna parte puede contener. Escapar la cadena ya unida no
/// bastaba: el separador era indistinguible de un separador dentro de una
/// parte, y `("a|b", "c")` y `("a", "b|c")` acababan en el mismo
/// registro.
fn escape_id_part(value: &str) -> String {
    value
        .replace('~', "~0")
        .replace(':', "~1")
        .replace('|', "~2")
}

/// El identificador compuesto de unas coordenadas.
///
/// Las partes se escapan una a una y se unen con `|`, que ya no puede
/// aparecer dentro de una parte: la separacion es reversible y dos
/// coordenadas distintas nunca producen el mismo identificador.
fn composite_id(parts: &[&str]) -> String {
    parts
        .iter()
        .map(|part| escape_id_part(part))
        .collect::<Vec<String>>()
        .join("|")
}

/// El identificador del repositorio de una cuenta.
fn repository_id(account: &str, repo_key: &str) -> Thing {
    Thing::from((
        "catalog_repository",
        composite_id(&[account, repo_key]).as_str(),
    ))
}

/// El identificador de la version publicada.
fn version_id(account: &str, repository: &str, package_name: &str, package_version: &str) -> Thing {
    Thing::from((
        "catalog_version",
        composite_id(&[account, repository, package_name, package_version]).as_str(),
    ))
}

/// El identificador del asset de una ruta.
fn asset_id(account: &str, repository: &str, asset_path: &str) -> Thing {
    Thing::from((
        "catalog_asset",
        composite_id(&[account, repository, asset_path]).as_str(),
    ))
}

/// El identificador de la referencia al blob de una ruta.
fn reference_id(account: &str, repository: &str, asset_path: &str) -> Thing {
    Thing::from((
        "catalog_reference",
        composite_id(&[account, repository, asset_path]).as_str(),
    ))
}

/// El identificador de una etiqueta.
fn tag_id(account: &str, repository: &str, tag_name: &str) -> Thing {
    Thing::from((
        "catalog_tag",
        composite_id(&[account, repository, tag_name]).as_str(),
    ))
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
    ///
    /// No consulta al autorizador, y no es un olvido: esto no es una
    /// operacion del plano de datos, es administracion del almacen. No
    /// habla de cuentas ni de repositorios, y su unico efecto es el
    /// esquema y el numero de version que dice como esta.
    pub async fn initialize(&self) -> Result<(), CatalogError> {
        // Antes de declarar nada, se mira que version dice el almacen.
        // Declarar el esquema es idempotente, pero **declarar el de esta
        // build encima del de una build mas nueva** no deja el almacen en
        // la version nueva: las definiciones que ya estan no se rehacen,
        // asi que el almacen se queda con un esquema mixto y el numero de
        // version que se escribe debajo miente. Un almacen mas nuevo que
        // esta build es un almacen que esta build no debe tocar.
        let applied = self.applied_schema_version().await?;
        if applied > SCHEMA_VERSION {
            return Err(CatalogError::SchemaMismatch {
                found: applied,
                expected: SCHEMA_VERSION,
            });
        }

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
    ///
    /// Como `initialize`, esto es administracion del almacen y no una
    /// operacion del plano de datos: no consulta al autorizador porque no
    /// habla de cuentas ni de repositorios.
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
    ///
    /// La decision se toma **antes** de tocar la base de datos: si el
    /// decisor dice que no, no se llega a preguntar si la cosa existe. De
    /// lo contrario, un `Denied` sobre un objeto de otra cuenta seria una
    /// forma de filtrar su existencia.
    async fn decide(&self, request: &AccessRequest) -> Result<(), CatalogError> {
        match self.authorizer.authorize(request).await? {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny(reason) => {
                // La clave dice que cosa se pidio, no solo el repositorio:
                // un `Denied` que dice "asset" cuando lo que se pidio fue
                // un repositorio hace que el cliente ramifique por una
                // clase equivocada.
                let key = match request.resource {
                    CatalogResource::Repository => request.subject.clone(),
                    _ => format!("{}/{}", request.repository, request.subject),
                };
                Err(CatalogError::Denied {
                    action: request.action.as_str(),
                    kind: request.resource.as_str(),
                    key,
                    reason,
                })
            }
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

/// Una fila minima, para las preguntas que solo necesitan un numero.
#[derive(Debug, Clone, Deserialize)]
struct CountRow {
    total: i64,
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
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Read,
            repo_key,
        ))
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
        self.decide(&AccessRequest::asset(
            principal_account,
            CatalogAction::Read,
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
        if reference.size_bytes != asset.size_bytes {
            return Err(CatalogError::corrupt(
                "asset",
                key,
                "the asset and its blob reference disagree about the size of the content",
            ));
        }

        // Y los bytes tienen que ser los que el catalogo dice. La pregunta
        // lleva la ubicacion y no solo el digest: los bytes pueden estar
        // en la ranura global o en la particionada de la cuenta, y
        // preguntar por el digest a secas solo encontraria la primera.
        if let Some(probe) = &self.probe {
            let location =
                BlobLocation::new(principal_account, reference.scope, asset.digest.clone());
            match probe.length_of(&location).await {
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
        self.decide(&AccessRequest::version(
            principal_account,
            CatalogAction::Read,
            repository,
            coordinates,
        ))
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
        self.decide(&AccessRequest::tag(
            principal_account,
            CatalogAction::Read,
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
        self.decide(&AccessRequest::asset(
            principal_account,
            CatalogAction::Read,
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
    /// Si la configuracion del repositorio cambio mientras se actualizaba.
    ///
    /// Se relee el repositorio: si su version ya no es la que se leyo, otro
    /// cambio se colo y esta actualizacion no se puede aplicar encima.
    async fn the_configuration_moved(
        &self,
        principal_account: &str,
        repo_key: &str,
        previous: i64,
    ) -> Result<bool, CatalogError> {
        Ok(self
            .the_repository_row(principal_account, repo_key)
            .await?
            .is_some_and(|row| row.config_version != previous))
    }

    /// Si el repositorio tiene alguna version publicada.
    ///
    /// No pregunta por cual: solo por si hay contenido, que es lo que
    /// decide si la politica del repositorio todavia se puede cambiar.
    async fn the_version_row_any(
        &self,
        principal_account: &str,
        repository: &str,
    ) -> Result<bool, CatalogError> {
        let mut response = self
            .db
            .query(
                "SELECT count() AS total FROM catalog_version \
                 WHERE account = $account AND repository = $repository",
            )
            .bind(("account", principal_account.to_string()))
            .bind(("repository", repository.to_string()))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let rows: Vec<CountRow> = response
            .take(0)
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(rows.first().is_some_and(|row| row.total > 0))
    }

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
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Configure,
            &descriptor.repo_key,
        ))
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
        // El nombre canonico tiene que ser un HRN que la plataforma sepa
        // leer. Un repositorio cuyo nombre canonico no se puede parsear es
        // un repositorio que el resto de la plataforma no puede nombrar,
        // y eso se dice ahora y no el dia que una politica lo pregunte.
        if Hrn::from_string(&descriptor.hrn.to_string()).is_none() {
            return Err(CatalogError::invalid(
                "the canonical name of the repository is not a valid HRN v2",
            ));
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

        let created = self
            .db
            .query("CREATE $id CONTENT $row")
            .bind((
                "id",
                repository_id(&descriptor.account, &descriptor.repo_key),
            ))
            .bind(("row", row))
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?
            .check();

        if let Err(error) = created {
            // La escritura no ha pasado. Antes de culpar al almacenamiento
            // se mira lo que hay: puede que el repositorio ya existiera, y
            // entonces la pregunta no es "ha fallado" sino "es el mismo".
            if let Some(existing) = self
                .the_repository_row(principal_account, &descriptor.repo_key)
                .await?
            {
                let existing = existing.into_descriptor()?;
                if same_repository(&existing, &descriptor) {
                    return Ok(existing);
                }
                return Err(CatalogError::conflict(
                    "repository",
                    descriptor.repo_key.clone(),
                    "there is already a repository with that key and another configuration",
                ));
            }
            return Err(CatalogError::unavailable(error.to_string()));
        }

        // La lectura de vuelta no decide otra vez: la escritura ya se
        // autorizo, y releer por el camino publico exigiria un permiso de
        // lectura que el que escribe no tiene por que tener.
        self.the_repository_row(principal_account, &descriptor.repo_key)
            .await?
            .ok_or_else(|| {
                CatalogError::unavailable("the repository was created and then it was not there")
            })?
            .into_descriptor()
    }

    async fn update_config(
        &self,
        principal_account: &str,
        repo_key: &str,
        config: RepositoryConfig,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Configure,
            repo_key,
        ))
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

        // Un repositorio que esta release no sirve no se reconfigura: no se
        // puede prometer el comportamiento de un proxy que no se sabe
        // resolver.
        if !current.mode.is_served_by_this_release() {
            return Err(CatalogError::ModeNotImplemented { mode: current.mode });
        }

        // El ambito de los digests decide donde viven los bytes, y los
        // bytes ya publicados viven donde se publicaron. Cambiar el
        // ambito de un repositorio que ya tiene contenido dejaria sus
        // blobs en una ranura que el catalogo ya no nombra, y la
        // recoleccion no sabria que siguen vivos.
        if config.digest_scope != current.config.digest_scope {
            let published = self
                .the_version_row_any(principal_account, repo_key)
                .await?;
            if published {
                return Err(CatalogError::conflict(
                    "repository",
                    repo_key.to_string(),
                    "the scope of the digests cannot change in a repository that already has content",
                ));
            }
        }

        // La version de la configuracion sube siempre: dos configuraciones
        // con la misma version tienen que ser la misma configuracion. Por
        // eso la escritura compara con la version que se leyo: si otro
        // cambio ocurrio entre la lectura y la escritura, esta no se
        // aplica y se dice que hubo un conflicto, en lugar de dejar dos
        // configuraciones distintas con el mismo numero.
        let version = i64::try_from(current.config.version.saturating_add(1)).unwrap_or(i64::MAX);
        let previous = i64::try_from(current.config.version).unwrap_or(0);

        let outcome = self
            .db
            .query(the_configuration_update())
            .bind(("id", repository_id(principal_account, repo_key)))
            .bind(("version", version))
            .bind(("previous", previous))
            .bind(("scope", scope_name(config.digest_scope)))
            .bind(("settings", config.config_settings.clone()))
            .bind(("at", Datetime::from(Utc::now())))
            .await;

        let rows: Vec<RepositoryRow> = match outcome {
            Ok(mut response) => response
                .take(0)
                .map_err(|error| CatalogError::unavailable(error.to_string()))?,
            Err(error) => {
                // Puede que otro escritor haya cambiado la configuracion
                // entre la lectura y la escritura. Eso no es un fallo del
                // almacenamiento: es un conflicto, y se dice como tal.
                if self
                    .the_configuration_moved(principal_account, repo_key, previous)
                    .await?
                {
                    return Err(CatalogError::conflict(
                        "repository",
                        repo_key.to_string(),
                        "the configuration changed while it was being updated",
                    ));
                }
                return Err(CatalogError::unavailable(error.to_string()));
            }
        };

        let rows = rows.into_iter().next().ok_or_else(|| {
            CatalogError::conflict(
                "repository",
                repo_key.to_string(),
                "the configuration changed while it was being updated",
            )
        })?;
        rows.into_descriptor()
    }

    async fn publish(
        &self,
        principal_account: &str,
        publication: Publication,
    ) -> Result<PublishOutcome, CatalogError> {
        // Se decide **antes** de validar: un principal sin permiso no
        // llega a saber si lo que pide esta bien formado, y la frontera de
        // permisos no se cruza por el camino de la validacion.
        self.decide(&AccessRequest::publish(
            principal_account,
            &publication.repository,
            &publication.asset_path,
        ))
        .await?;

        if publication.repository.trim().is_empty() {
            return Err(CatalogError::invalid("the repository is empty"));
        }
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

        // El contenido tiene que estar en el almacen de bytes. Publicar
        // una referencia a un blob que no existe deja un camino que no
        // lleva a nada, y el catalogo que afirma una integridad que no
        // verifica es un catalogo que miente.
        //
        // Sin sonda no se puede comprobar, y entonces el catalogo guarda
        // lo que se le declara: el almacen de bytes es quien responde si
        // los bytes estan, y esa frontera esta escrita en el puerto.
        if let Some(probe) = &self.probe {
            let location = BlobLocation::new(principal_account, scope, publication.digest.clone());
            match probe.length_of(&location).await {
                Ok(size) if size == publication.size_bytes => {}
                Ok(size) => {
                    return Err(CatalogError::invalid(format!(
                        "the content in the store has {size} bytes and the publication declares {}",
                        publication.size_bytes
                    )));
                }
                Err(BlobProbeError::Missing) => {
                    return Err(CatalogError::invalid(
                        "the content is not in the blob store",
                    ));
                }
                Err(BlobProbeError::Unavailable { detail }) => {
                    return Err(CatalogError::unavailable(detail));
                }
            }
        }

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
        self.decide(&AccessRequest::tag(
            principal_account,
            CatalogAction::MoveTag,
            repository,
            tag_name,
        ))
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
        // La etiqueta se relee por dentro: la escritura ya se autorizo, y
        // releer por el camino publico exigiria un permiso de lectura que
        // el que mueve la etiqueta no tiene por que tener.
        self.the_tag_row(principal_account, repository, tag_name)
            .await?
            .ok_or_else(|| {
                CatalogError::unavailable("the tag was written and then it was not there")
            })?
            .into_tag()
    }

    async fn move_asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
        coordinates: PackageCoordinates,
    ) -> Result<AssetRecord, CatalogError> {
        self.decide(&AccessRequest::asset(
            principal_account,
            CatalogAction::MoveAsset,
            repository,
            asset_path,
        ))
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

        // El ambito es el del contenido que ya estaba ahi, no el de la
        // configuracion vigente: los bytes se publicaron bajo un ambito, y
        // ese es el ambito en el que siguen viviendo.
        let scope = reference.scope;
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
        // tiene que decir lo mismo que el. Las dos filas se releen por
        // dentro: la escritura ya se autorizo, y releer por el camino
        // publico exigiria un permiso de lectura que el que mueve el
        // asset no tiene por que tener.
        let moved = self
            .the_asset_row(principal_account, repository, asset_path)
            .await?
            .ok_or_else(|| {
                CatalogError::unavailable("the asset was moved and then it was not there")
            })?
            .into_asset()?;
        if moved.digest != version.digest {
            return Err(CatalogError::corrupt(
                "asset",
                format!("{repository}/{asset_path}"),
                "the asset does not point at the version it was moved to",
            ));
        }
        let moved_reference = self
            .the_reference_row(principal_account, repository, asset_path)
            .await?
            .ok_or_else(|| {
                CatalogError::unavailable("the reference was moved and then it was not there")
            })?
            .into_reference()?;
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
        let mut asset = AssetRecord {
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
                // Lo que no se escribe no se actualiza. El resultado que
                // se devuelve dice lo que hay en la base, no un instante
                // que la base nunca tuvo: una republicacion idempotente
                // que dice "actualizado ahora" miente sobre cuando se
                // publico el contenido.
                asset.updated_at = DateTime::<Utc>::from(row.updated_at.clone());
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
        let mut reference = BlobReference {
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
            // La identidad del contenido es el digest. El ambito dice
            // donde viven los bytes, no que bytes son: compararlo aqui
            // haria que un cambio de politica pareciera un conflicto de
            // contenido cuando el contenido es el mismo.
            let same = row.digest == digest.value
                && row.digest_algorithm == algorithm_of(&digest)
                && row.size_bytes == size_bytes;
            if same {
                create_reference = false;
                // Como con el asset: lo que no se escribe no cambia de
                // fecha, y el resultado dice lo que hay.
                reference.updated_at = DateTime::<Utc>::from(row.updated_at.clone());
            } else if conflict.is_none() {
                conflict = Some(CatalogError::conflict(
                    "reference",
                    format!("{repository}/{}", publication.asset_path),
                    "the path already references another blob",
                ));
            }
        }

        // Las etiquetas. La misma etiqueta dos veces en una publicacion es
        // la misma etiqueta: se escribe una, no dos.
        let mut create_tags: Vec<String> = Vec::new();
        for tag_name in &publication.tags {
            if tag_name.trim().is_empty() {
                return Err(CatalogError::invalid("a tag name is empty"));
            }
            if create_tags.iter().any(|seen| seen == tag_name) {
                continue;
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

/// La escritura de la configuracion de un repositorio.
///
/// Es un compare-and-set: dice en que version parte y solo se aplica si la
/// version que hay en la base es esa. Sin esa condicion, dos
/// reconfiguraciones simultaneas podrian quedarse las dos con el mismo
/// numero de version y ajustes distintos, y el numero dejaria de decir
/// que configuracion es la que hay.
fn the_configuration_update() -> &'static str {
    "UPDATE $id SET config_version = $version, digest_scope = $scope, \
     config_settings = $settings, updated_at = $at \
     WHERE config_version = $previous \
     RETURN account, repo_key, hrn, repo_format, repo_mode, config_version, \
     digest_scope, config_settings, created_at, updated_at"
}

/// Si dos repositorios son el mismo.
///
/// La comparacion es de contenido y no de momento: dos llamadas que piden
/// el mismo repositorio con la misma configuracion describen la misma
/// cosa, y la segunda no es un error sino la misma respuesta. Si algo
/// difiere, entonces si son dos repositorios distintos y la segunda
/// llamada es un conflicto.
fn same_repository(existing: &RepositoryDescriptor, requested: &RepositoryDescriptor) -> bool {
    existing.account == requested.account
        && existing.repo_key == requested.repo_key
        && existing.hrn == requested.hrn
        && existing.format == requested.format
        && existing.mode == requested.mode
        && existing.config.version == requested.config.version
        && existing.config.digest_scope == requested.config.digest_scope
        && existing.config.config_settings == requested.config.config_settings
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

#[cfg(test)]
mod tests {
    use super::*;
    use hodei_catalog::{
        AllowAllAuthorizer, RepositoryConfig, RepositoryDescriptor, RepositoryFormat,
        RepositoryMode, repository_hrn,
    };
    use surrealdb::engine::local::RocksDb;

    /// Un almacen con su base, para las pruebas del propio adaptador.
    async fn a_store() -> (SurrealCatalogStore, tempfile::TempDir) {
        let directory = tempfile::tempdir().expect("temp dir");
        let db = Surreal::new::<RocksDb>(directory.path().join("db"))
            .await
            .expect("open the database");
        db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
        let store = SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer));
        store.initialize().await.expect("initialize");
        (store, directory)
    }

    /// La escritura de la configuracion solo se aplica sobre la version que
    /// se leyo.
    ///
    /// Sin la condicion, dos reconfiguraciones simultaneas podrian quedarse
    /// las dos con el mismo numero de version y ajustes distintos, y el
    /// numero dejaria de decir que configuracion es la que hay. Esta
    /// prueba lo comprueba sobre la escritura de verdad, y no sobre una
    /// carrera que depende del calendario.
    #[tokio::test]
    async fn the_configuration_is_only_written_over_the_version_that_was_read() {
        let (store, _directory) = a_store().await;
        store
            .create_repository(
                "acme",
                RepositoryDescriptor {
                    hrn: repository_hrn("hodei", "artifacts", "eu", "acme", "releases"),
                    account: "acme".to_string(),
                    repo_key: "releases".to_string(),
                    format: RepositoryFormat::Raw,
                    mode: RepositoryMode::Hosted,
                    config: RepositoryConfig::new(DigestScope::Global),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                },
            )
            .await
            .expect("create repository");

        // Una escritura que parte de una version que no es la de ahora no
        // se aplica: no toca ninguna fila.
        let mut stale = store
            .db
            .query(the_configuration_update())
            .bind(("id", repository_id("acme", "releases")))
            .bind(("version", 99))
            .bind(("previous", 0))
            .bind(("scope", scope_name(DigestScope::Global)))
            .bind(("settings", serde_json::json!({ "stale": true })))
            .bind(("at", Datetime::from(Utc::now())))
            .await
            .expect("the query runs");
        let rows: Vec<RepositoryRow> = stale.take(0).expect("the rows");
        assert!(
            rows.is_empty(),
            "a stale update changed the configuration of the repository"
        );

        // Y el repositorio sigue como estaba.
        let repository = store
            .repository("acme", "releases")
            .await
            .expect("read repository");
        assert_eq!(repository.config.version, 1);
        assert!(
            !repository.config.config_settings.get("stale").is_some(),
            "a stale update changed the settings of the repository"
        );

        // Una escritura que parte de la version de ahora si se aplica.
        let mut fresh = store
            .db
            .query(the_configuration_update())
            .bind(("id", repository_id("acme", "releases")))
            .bind(("version", 2))
            .bind(("previous", 1))
            .bind(("scope", scope_name(DigestScope::Global)))
            .bind(("settings", serde_json::json!({ "fresh": true })))
            .bind(("at", Datetime::from(Utc::now())))
            .await
            .expect("the query runs");
        let rows: Vec<RepositoryRow> = fresh.take(0).expect("the rows");
        assert_eq!(rows.len(), 1, "the fresh update did not apply");
    }

    /// El identificador de un registro no depende de como se parta.
    ///
    /// Dos coordenadas que solo se distinguen por el separador tienen que
    /// dar dos registros distintos, y no uno: un identificador que no sabe
    /// distinguir dos cosas hace que una pise a la otra.
    #[test]
    fn two_coordinates_that_only_differ_in_the_separator_have_two_identifiers() {
        let first = composite_id(&["acme", "releases", "pkg|1", "0"]);
        let second = composite_id(&["acme", "releases", "pkg", "1|0"]);
        assert_ne!(
            first, second,
            "two different coordinates share one identifier: {first}"
        );
    }
}
