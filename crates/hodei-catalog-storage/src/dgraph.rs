//! El puerto Dgraph del catálogo (HSP-187, slice Raw).
//!
//! Porta el catálogo durable de HAR-02/HAR-03 sobre el motor de grafo
//! nativo (`hodei-graph-client`): Repository, Asset, Version,
//! BlobReference y el outbox de auditoría, manteniendo el CAS **externo**
//! — los bytes nunca viven en el grafo; el grafo apunta por digest.
//!
//! La frontera de commit es la que marca HAR-03: **el commit del catálogo
//! y el evento de auditoría son UNA transacción**. La transacción es el
//! upsert atómico de la frontera de proyección (HSP-199): la guarda de
//! ámbito (`hodei.guard.*`) avanza, el marcador de operación
//! (`hodei.operation.*`) se escribe una sola vez y las filas del catálogo
//! más el evento se aplican en bloques condicionados del mismo commit.
//! La respuesta la decide **el estado después del commit**, nunca la
//! esperanza de que la transacción haya commiteado.
//!
//! Las garantías por clave: cada fila lleva su clave compuesta
//! (`hodei.catalog.*.key`, índice exacto y `@upsert`), así que el servidor
//! —no un pre-lectura— arbitra dos escritores de la misma clave. El
//! idempotente es el de HAR-02: republicar lo mismo no es un error ni una
//! escritura nueva, y un evento que dice "publicado" sobre lo que ya
//! estaba no añade ningún hecho.
//!
//! El slice es **Raw**: repository, asset, versión, referencia y outbox.
//! Lo que otro formato necesita (etiquetas mutables, reconfiguración de
//! repositorios, vistas derivadas) no está portado aquí y lo dice con un
//! error tipado en vez de fingir que lo sirve.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use hodei_audit::{
    AuditContext, AuditEntry, AuditError, AuditEvent, AuditOutbox, AuditResult, EntryState,
};
use hodei_catalog::{
    AccessDecision, AccessRequest, AssetDisposition, AssetRecord, BlobIntegrityProbe, BlobLocation,
    BlobProbeError, BlobReference, CatalogAction, CatalogAuthorizer, CatalogError, CatalogReader,
    CatalogResource, CatalogWriter, DigestScope, MutableTag, PackageCoordinates, PackageVersion,
    Precondition, Publication, PublishOutcome, ReferrerRecord, RepositoryConfig,
    RepositoryDescriptor, RepositoryFormat, RepositoryMode, TrustState,
};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_graph_client::guard::{GuardKind, GuardScope, GuardedEffect};
use hodei_graph_client::{
    GraphClient, GraphClientError, GraphMutationBlock, GraphMutationPort, GuardStore, OperationId,
    RevisionDemand, Tenant, dql_string,
};
use kernel::application::ports::clock::Identity;
use sha2::{Digest as _, Sha256};

/// Cuántas veces la publicación relee y reintenta antes de rendirse.
const WRITE_ATTEMPTS: usize = 6;

/// El tipo de grafo de una fila de repositorio.
pub const REPOSITORY_TYPE: &str = "CatalogRepository";
/// El tipo de grafo de un asset.
pub const ASSET_TYPE: &str = "CatalogAsset";
/// El tipo de grafo de una versión publicada.
pub const VERSION_TYPE: &str = "CatalogVersion";
/// El tipo de grafo de una referencia viva a un blob del CAS.
pub const REFERENCE_TYPE: &str = "CatalogReference";
/// El tipo de grafo de una fila del outbox de auditoría.
pub const OUTBOX_TYPE: &str = "CatalogAuditOutbox";

/// Versión del esquema **propio** del catálogo sobre Dgraph. Va aparte del
/// esquema versionado del grafo (`hodei-graph-client`, v7): este slice
/// declara sus predicados al abrir el almacén, y no toca el contrato de
/// arranque que el resto del stack valida.
pub const CATALOG_DGRAPH_SCHEMA_VERSION: i64 = 1;

/// El DDL aditivo del catálogo sobre Dgraph. Cada clave es índice exacto y
/// `@upsert`: el servidor aborta a quien escriba una segunda fila con la
/// misma clave, dentro de la misma transacción o entre transacciones.
pub fn catalog_schema_dql() -> String {
    let mut ddl = String::new();
    for (predicate, kind, index) in [
        (
            "hodei.catalog.repository.key",
            "string",
            "@index(exact) @upsert",
        ),
        (
            "hodei.catalog.repository.account",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.repository.repo_key",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.repository.hrn", "string", ""),
        ("hodei.catalog.repository.format", "string", "@index(exact)"),
        ("hodei.catalog.repository.mode", "string", "@index(exact)"),
        (
            "hodei.catalog.repository.config_version",
            "int",
            "@index(int)",
        ),
        ("hodei.catalog.repository.config_scope", "string", ""),
        ("hodei.catalog.repository.config_settings", "string", ""),
        ("hodei.catalog.repository.created_at", "datetime", ""),
        ("hodei.catalog.repository.updated_at", "datetime", ""),
        ("hodei.catalog.asset.key", "string", "@index(exact) @upsert"),
        ("hodei.catalog.asset.account", "string", "@index(exact)"),
        ("hodei.catalog.asset.repository", "string", "@index(exact)"),
        ("hodei.catalog.asset.asset_path", "string", "@index(exact)"),
        (
            "hodei.catalog.asset.package_name",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.asset.package_version",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.asset.digest_algorithm",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.asset.digest", "string", "@index(exact)"),
        ("hodei.catalog.asset.size_bytes", "int", ""),
        ("hodei.catalog.asset.subject", "string", "@index(exact)"),
        ("hodei.catalog.asset.updated_at", "datetime", ""),
        (
            "hodei.catalog.version.key",
            "string",
            "@index(exact) @upsert",
        ),
        ("hodei.catalog.version.account", "string", "@index(exact)"),
        (
            "hodei.catalog.version.repository",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.version.package_name",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.version.package_version",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.version.digest_algorithm",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.version.digest", "string", "@index(exact)"),
        ("hodei.catalog.version.size_bytes", "int", ""),
        ("hodei.catalog.version.published_at", "datetime", ""),
        (
            "hodei.catalog.reference.key",
            "string",
            "@index(exact) @upsert",
        ),
        ("hodei.catalog.reference.account", "string", "@index(exact)"),
        (
            "hodei.catalog.reference.repository",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.reference.asset_path",
            "string",
            "@index(exact)",
        ),
        (
            "hodei.catalog.reference.digest_algorithm",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.reference.digest", "string", "@index(exact)"),
        ("hodei.catalog.reference.scope", "string", "@index(exact)"),
        ("hodei.catalog.reference.size_bytes", "int", ""),
        ("hodei.catalog.reference.trust_state", "string", ""),
        ("hodei.catalog.reference.updated_at", "datetime", ""),
        (
            "hodei.catalog.outbox.key",
            "string",
            "@index(exact) @upsert",
        ),
        ("hodei.catalog.outbox.tenant", "string", "@index(exact)"),
        ("hodei.catalog.outbox.actor", "string", ""),
        ("hodei.catalog.outbox.correlation_id", "string", ""),
        ("hodei.catalog.outbox.action", "string", ""),
        ("hodei.catalog.outbox.resource", "string", ""),
        ("hodei.catalog.outbox.digest", "string", ""),
        ("hodei.catalog.outbox.result", "string", ""),
        ("hodei.catalog.outbox.occurred_at", "datetime", ""),
        ("hodei.catalog.outbox.state", "string", "@index(exact)"),
        ("hodei.catalog.outbox.attempts", "int", ""),
        (
            "hodei.catalog.outbox.lease_owner",
            "string",
            "@index(exact)",
        ),
        ("hodei.catalog.outbox.lease_until", "datetime", ""),
        ("hodei.catalog.outbox.last_error", "string", ""),
        ("hodei.catalog.outbox.delivered_at", "datetime", ""),
    ] {
        ddl.push_str(predicate);
        ddl.push_str(": ");
        ddl.push_str(kind);
        if !index.is_empty() {
            ddl.push(' ');
            ddl.push_str(index);
        }
        ddl.push_str(" .\n");
    }
    for (name, family, fields) in [
        (
            REPOSITORY_TYPE,
            "repository",
            "key account repo_key hrn format mode config_version config_scope \
             config_settings created_at updated_at",
        ),
        (
            ASSET_TYPE,
            "asset",
            "key account repository asset_path package_name package_version \
             digest_algorithm digest size_bytes subject updated_at",
        ),
        (
            VERSION_TYPE,
            "version",
            "key account repository package_name package_version digest_algorithm \
             digest size_bytes published_at",
        ),
        (
            REFERENCE_TYPE,
            "reference",
            "key account repository asset_path digest_algorithm digest scope \
             size_bytes trust_state updated_at",
        ),
        (
            OUTBOX_TYPE,
            "outbox",
            "key tenant actor correlation_id action resource digest result \
             occurred_at state attempts lease_owner lease_until last_error delivered_at",
        ),
    ] {
        ddl.push_str(&format!("type {name} {{\n"));
        for field in fields.split_whitespace() {
            ddl.push_str(&format!("hodei.catalog.{family}.{field}\n"));
        }
        ddl.push_str("}\n");
    }
    ddl
}

/// Escapa una parte de una clave compuesta: el separador `|` y el `@` de
/// las coordenadas no pueden aparecer tal cual dentro de una parte, o dos
/// partes distintas producirían la misma clave.
fn key_part(value: &str) -> String {
    value
        .replace('%', "%25")
        .replace('|', "%7C")
        .replace('@', "%40")
}

/// La clave compuesta de un repositorio.
pub fn repository_key(account: &str, repo_key: &str) -> String {
    format!("{}|{}", key_part(account), key_part(repo_key))
}

/// La clave compuesta de un asset (y de su referencia: la referencia es la
/// fila que se mueve con el asset).
pub fn asset_key(account: &str, repository: &str, asset_path: &str) -> String {
    format!(
        "{}|{}|{}",
        key_part(account),
        key_part(repository),
        key_part(asset_path)
    )
}

/// La clave compuesta de una versión publicada.
pub fn version_key(
    account: &str,
    repository: &str,
    package_name: &str,
    package_version: &str,
) -> String {
    format!(
        "{}|{}|v|{}@{}",
        key_part(account),
        key_part(repository),
        key_part(package_name),
        key_part(package_version)
    )
}

/// El sujeto de la guarda de una publicación: el camino del asset dentro
/// de la cuenta. Dos escritores de la misma ruta —de donde venga el
/// código— derivan el mismo sujeto y por eso se pelean por la misma
/// guarda; dos rutas distintas nunca colisionan.
fn asset_guard_scope(
    account: &str,
    repository: &str,
    asset_path: &str,
) -> Result<GuardScope, CatalogError> {
    scope_of(
        GuardKind::CatalogAsset,
        account,
        &format!("asset/{repository}/{asset_path}"),
    )
}

/// El sujeto de la guarda de una creación de repositorio.
fn repository_guard_scope(account: &str, repo_key: &str) -> Result<GuardScope, CatalogError> {
    scope_of(
        GuardKind::CatalogRepository,
        account,
        &format!("repository/{repo_key}"),
    )
}

/// Valida el sujeto de una guarda: una clave que la guarda no puede
/// proteger (el separador, espacios, caracteres de control) no es una
/// clave que este slice publique.
fn scope_of(kind: GuardKind, tenant: &str, subject: &str) -> Result<GuardScope, CatalogError> {
    let tenant = Tenant::new(tenant).map_err(|error| {
        CatalogError::invalid(format!("the account is not a guard tenant: {error}"))
    })?;
    GuardScope::new(kind, &tenant, subject)
        .map_err(|error| CatalogError::invalid(format!("the key is not publishable here: {error}")))
}

/// El identificador de operación determinista de una publicación: el mismo
/// hecho dicho dos veces lleva el mismo identificador, así que el
/// reintento tras el commit relee el resultado comprometido en vez de
/// escribir por segunda vez.
fn publication_operation_id(account: &str, publication: &Publication) -> OperationId {
    let mut hasher = Sha256::new();
    // La cuenta entra en el hecho: la misma ruta en dos cuentas son dos
    // hechos distintos, y un marcador once-only es por grafo.
    hasher.update(account.as_bytes());
    hasher.update([0]);
    hasher.update(publication.repository.as_bytes());
    hasher.update([0]);
    hasher.update(publication.asset_path.as_bytes());
    hasher.update([0]);
    hasher.update(publication.coordinates.package_name.as_bytes());
    hasher.update([0]);
    hasher.update(publication.coordinates.package_version.as_bytes());
    hasher.update([0]);
    hasher.update(publication.digest.algorithm.to_string().as_bytes());
    hasher.update([0]);
    hasher.update(publication.digest.value.as_bytes());
    hasher.update([0]);
    hasher.update(publication.size_bytes.to_le_bytes());
    hasher.update([0]);
    hasher.update(if publication.overwrite { b"1" } else { b"0" });
    hasher.update([0]);
    hasher.update(publication.correlation_id.as_bytes());
    let id = format!("pub-{:x}", hasher.finalize());
    OperationId::new(&id).expect("a sha256-derived operation id is always valid")
}

/// El identificador de operación determinista de una creación de repositorio.
fn repository_operation_id(descriptor: &RepositoryDescriptor) -> OperationId {
    let mut hasher = Sha256::new();
    hasher.update(descriptor.account.as_bytes());
    hasher.update([0]);
    hasher.update(descriptor.repo_key.as_bytes());
    hasher.update([0]);
    hasher.update(descriptor.format.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(descriptor.mode.as_str().as_bytes());
    hasher.update([0]);
    hasher.update(
        match descriptor.config.digest_scope {
            DigestScope::Global => "global",
            DigestScope::AccountPartitioned => "account_partitioned",
        }
        .as_bytes(),
    );
    hasher.update([0]);
    hasher.update(descriptor.config.config_settings.to_string().as_bytes());
    let id = format!("repo-{:x}", hasher.finalize());
    OperationId::new(&id).expect("a sha256-derived operation id is always valid")
}

// ---------------------------------------------------------------------------
// Lectura de filas.
// ---------------------------------------------------------------------------

/// Los campos que lee una fila de repositorio.
const REPOSITORY_FIELDS: &str = "hodei.catalog.repository.key hodei.catalog.repository.account \
     hodei.catalog.repository.repo_key hodei.catalog.repository.hrn hodei.catalog.repository.format \
     hodei.catalog.repository.mode hodei.catalog.repository.config_version \
     hodei.catalog.repository.config_scope hodei.catalog.repository.config_settings \
     hodei.catalog.repository.created_at hodei.catalog.repository.updated_at";

/// Los campos que lee una fila de asset.
const ASSET_FIELDS: &str = "hodei.catalog.asset.key hodei.catalog.asset.account \
     hodei.catalog.asset.repository hodei.catalog.asset.asset_path \
     hodei.catalog.asset.package_name hodei.catalog.asset.package_version \
     hodei.catalog.asset.digest_algorithm hodei.catalog.asset.digest \
     hodei.catalog.asset.size_bytes hodei.catalog.asset.subject hodei.catalog.asset.updated_at";

/// Los campos que lee una fila de versión.
const VERSION_FIELDS: &str = "hodei.catalog.version.key hodei.catalog.version.account \
     hodei.catalog.version.repository hodei.catalog.version.package_name \
     hodei.catalog.version.package_version hodei.catalog.version.digest_algorithm \
     hodei.catalog.version.digest hodei.catalog.version.size_bytes \
     hodei.catalog.version.published_at";

/// Los campos que lee una fila de referencia.
const REFERENCE_FIELDS: &str = "hodei.catalog.reference.key hodei.catalog.reference.account \
     hodei.catalog.reference.repository hodei.catalog.reference.asset_path \
     hodei.catalog.reference.digest_algorithm hodei.catalog.reference.digest \
     hodei.catalog.reference.scope hodei.catalog.reference.size_bytes \
     hodei.catalog.reference.trust_state hodei.catalog.reference.updated_at";

/// Los campos que lee una fila del outbox.
const OUTBOX_FIELDS: &str = "hodei.catalog.outbox.key hodei.catalog.outbox.tenant \
     hodei.catalog.outbox.actor hodei.catalog.outbox.correlation_id hodei.catalog.outbox.action \
     hodei.catalog.outbox.resource hodei.catalog.outbox.digest hodei.catalog.outbox.result \
     hodei.catalog.outbox.occurred_at hodei.catalog.outbox.state hodei.catalog.outbox.attempts \
     hodei.catalog.outbox.lease_owner hodei.catalog.outbox.lease_until \
     hodei.catalog.outbox.last_error hodei.catalog.outbox.delivered_at";

/// Lee un texto de un nodo, o lo dice como corrupción.
fn node_text(node: &serde_json::Value, field: &str) -> Result<String, CatalogError> {
    node.get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| CatalogError::corrupt("row", field, format!("the node has no '{field}'")))
}

fn node_u64(node: &serde_json::Value, field: &str) -> Result<u64, CatalogError> {
    node.get(field)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| CatalogError::corrupt("row", field, format!("the node has no '{field}'")))
}

fn node_time(node: &serde_json::Value, field: &str) -> Result<DateTime<Utc>, CatalogError> {
    let text = node_text(node, field)?;
    DateTime::parse_from_rfc3339(&text)
        .map(|instant| instant.with_timezone(&Utc))
        .map_err(|_| CatalogError::corrupt("row", field, format!("'{text}' is not a timestamp")))
}

fn node_digest(
    node: &serde_json::Value,
    algorithm_field: &str,
    value_field: &str,
) -> Result<ContentDigest, CatalogError> {
    let algorithm = match node_text(node, algorithm_field)?.as_str() {
        "sha256" => DigestAlgorithm::Sha256,
        "sha512" => DigestAlgorithm::Sha512,
        other => {
            return Err(CatalogError::corrupt(
                "row",
                algorithm_field,
                format!("'{other}' is not a digest algorithm"),
            ));
        }
    };
    Ok(ContentDigest {
        algorithm,
        value: node_text(node, value_field)?,
    })
}

/// La fila de un repositorio tal como está en el grafo.
#[derive(Clone)]
struct RepositoryRow {
    account: String,
    repo_key: String,
    hrn: String,
    format: String,
    mode: String,
    config_version: u64,
    config_scope: String,
    config_settings: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl RepositoryRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        Ok(Self {
            account: node_text(node, "hodei.catalog.repository.account")?,
            repo_key: node_text(node, "hodei.catalog.repository.repo_key")?,
            hrn: node_text(node, "hodei.catalog.repository.hrn")?,
            format: node_text(node, "hodei.catalog.repository.format")?,
            mode: node_text(node, "hodei.catalog.repository.mode")?,
            config_version: node_u64(node, "hodei.catalog.repository.config_version")?,
            config_scope: node_text(node, "hodei.catalog.repository.config_scope")?,
            config_settings: node_text(node, "hodei.catalog.repository.config_settings")?,
            created_at: node_time(node, "hodei.catalog.repository.created_at")?,
            updated_at: node_time(node, "hodei.catalog.repository.updated_at")?,
        })
    }

    /// El descriptor de la fila, con la identidad comprobada contra lo que
    /// la clave dice que son.
    fn into_descriptor(
        self,
        account: &str,
        repo_key: &str,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        if self.account != account || self.repo_key != repo_key {
            return Err(CatalogError::corrupt(
                "repository",
                format!("{account}/{repo_key}"),
                "the row does not carry the identity its key names",
            ));
        }
        let format = RepositoryFormat::parse(&self.format).ok_or_else(|| {
            CatalogError::corrupt(
                "repository",
                format!("{account}/{repo_key}"),
                format!("'{}' is not a format", self.format),
            )
        })?;
        let mode = RepositoryMode::parse(&self.mode).ok_or_else(|| {
            CatalogError::corrupt(
                "repository",
                format!("{account}/{repo_key}"),
                format!("'{}' is not a mode", self.mode),
            )
        })?;
        let scope = match self.config_scope.as_str() {
            "global" => DigestScope::Global,
            "account_partitioned" => DigestScope::AccountPartitioned,
            other => {
                return Err(CatalogError::corrupt(
                    "repository",
                    format!("{account}/{repo_key}"),
                    format!("'{other}' is not a digest scope"),
                ));
            }
        };
        let settings: serde_json::Value = serde_json::from_str(&self.config_settings)
            .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
        let hrn = kernel::Hrn::from_string(&self.hrn).ok_or_else(|| {
            CatalogError::corrupt(
                "repository",
                format!("{account}/{repo_key}"),
                format!("'{}' is not an HRN", self.hrn),
            )
        })?;
        Ok(RepositoryDescriptor {
            hrn,
            account: self.account,
            repo_key: self.repo_key,
            format,
            mode,
            config: RepositoryConfig {
                version: self.config_version,
                digest_scope: scope,
                config_settings: settings,
            },
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// La fila de una versión publicada.
#[derive(Clone)]
struct VersionRow {
    account: String,
    repository: String,
    package_name: String,
    package_version: String,
    digest: ContentDigest,
    size_bytes: u64,
    published_at: DateTime<Utc>,
}

impl VersionRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        Ok(Self {
            account: node_text(node, "hodei.catalog.version.account")?,
            repository: node_text(node, "hodei.catalog.version.repository")?,
            package_name: node_text(node, "hodei.catalog.version.package_name")?,
            package_version: node_text(node, "hodei.catalog.version.package_version")?,
            digest: node_digest(
                node,
                "hodei.catalog.version.digest_algorithm",
                "hodei.catalog.version.digest",
            )?,
            size_bytes: node_u64(node, "hodei.catalog.version.size_bytes")?,
            published_at: node_time(node, "hodei.catalog.version.published_at")?,
        })
    }

    fn into_package_version(self) -> PackageVersion {
        PackageVersion {
            account: self.account,
            repository: self.repository,
            coordinates: PackageCoordinates {
                package_name: self.package_name,
                package_version: self.package_version,
            },
            digest: self.digest,
            size_bytes: self.size_bytes,
            published_at: self.published_at,
        }
    }
}

/// La fila de un asset.
#[derive(Clone)]
struct AssetRow {
    account: String,
    repository: String,
    asset_path: String,
    package_name: String,
    package_version: String,
    digest: ContentDigest,
    size_bytes: u64,
    subject: Option<String>,
    updated_at: DateTime<Utc>,
}

impl AssetRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        let subject = node
            .get("hodei.catalog.asset.subject")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);
        Ok(Self {
            account: node_text(node, "hodei.catalog.asset.account")?,
            repository: node_text(node, "hodei.catalog.asset.repository")?,
            asset_path: node_text(node, "hodei.catalog.asset.asset_path")?,
            package_name: node_text(node, "hodei.catalog.asset.package_name")?,
            package_version: node_text(node, "hodei.catalog.asset.package_version")?,
            digest: node_digest(
                node,
                "hodei.catalog.asset.digest_algorithm",
                "hodei.catalog.asset.digest",
            )?,
            size_bytes: node_u64(node, "hodei.catalog.asset.size_bytes")?,
            subject,
            updated_at: node_time(node, "hodei.catalog.asset.updated_at")?,
        })
    }

    /// El registro del asset, con la identidad comprobada contra lo que
    /// la clave dice: una fila que no se llama como su clave está rota.
    fn into_record(
        self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<AssetRecord, CatalogError> {
        if self.account != account || self.repository != repository || self.asset_path != asset_path
        {
            return Err(CatalogError::corrupt(
                "asset",
                format!("{repository}/{asset_path}"),
                "the row does not carry the identity its key names",
            ));
        }
        let subject = self.subject.map(|value| ContentDigest {
            algorithm: DigestAlgorithm::Sha256,
            value,
        });
        Ok(AssetRecord {
            account: self.account,
            repository: self.repository,
            asset_path: self.asset_path,
            coordinates: PackageCoordinates {
                package_name: self.package_name,
                package_version: self.package_version,
            },
            digest: self.digest,
            size_bytes: self.size_bytes,
            subject,
            updated_at: self.updated_at,
        })
    }
}

/// La fila de una referencia viva a un blob.
#[derive(Clone)]
struct ReferenceRow {
    digest: ContentDigest,
    scope: DigestScope,
    size_bytes: u64,
    trust_state: TrustState,
    updated_at: DateTime<Utc>,
}

impl ReferenceRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        let scope = match node_text(node, "hodei.catalog.reference.scope")?.as_str() {
            "global" => DigestScope::Global,
            "account_partitioned" => DigestScope::AccountPartitioned,
            other => {
                return Err(CatalogError::corrupt(
                    "reference",
                    "scope",
                    format!("'{other}' is not a digest scope"),
                ));
            }
        };
        Ok(Self {
            digest: node_digest(
                node,
                "hodei.catalog.reference.digest_algorithm",
                "hodei.catalog.reference.digest",
            )?,
            scope,
            size_bytes: node_u64(node, "hodei.catalog.reference.size_bytes")?,
            trust_state: TrustState::of(&node_text(node, "hodei.catalog.reference.trust_state")?),
            updated_at: node_time(node, "hodei.catalog.reference.updated_at")?,
        })
    }

    fn into_reference(
        self,
        account: String,
        repository: String,
        asset_path: String,
    ) -> BlobReference {
        BlobReference {
            account,
            repository,
            asset_path,
            digest: self.digest,
            scope: self.scope,
            size_bytes: self.size_bytes,
            trust_state: self.trust_state,
            updated_at: self.updated_at,
        }
    }
}

// ---------------------------------------------------------------------------
// El estado de una publicación, pre-leído en una pregunta.
// ---------------------------------------------------------------------------

/// Lo que hay en el grafo para una publicación.
struct PublicationState {
    repository: Option<RepositoryRow>,
    version: Option<VersionRow>,
    asset: Option<AssetRow>,
    reference: Option<ReferenceRow>,
    event: Option<OutboxRow>,
}

/// Lee el estado de una publicación en una sola pregunta: repositorio,
/// versión, asset, referencia y la fila del evento que la publicación va
/// a decir. El evento se lee aquí para que una fila ya dicha con otro
/// contenido se descubra **antes** de escribir nada.
async fn read_publication_state(
    client: &Arc<GraphClient>,
    account: &str,
    publication: &Publication,
    event_id: &str,
) -> Result<PublicationState, CatalogError> {
    let repo = repository_key(account, &publication.repository);
    let version = version_key(
        account,
        &publication.repository,
        &publication.coordinates.package_name,
        &publication.coordinates.package_version,
    );
    let asset = asset_key(account, &publication.repository, &publication.asset_path);
    let query = format!(
        "{{\n \
         r(func: eq(hodei.catalog.repository.key, {})) {{ {REPOSITORY_FIELDS} }}\n \
         v(func: eq(hodei.catalog.version.key, {})) {{ {VERSION_FIELDS} }}\n \
         a(func: eq(hodei.catalog.asset.key, {})) {{ {ASSET_FIELDS} }}\n \
         rf(func: eq(hodei.catalog.reference.key, {})) {{ {REFERENCE_FIELDS} }}\n \
         ev(func: eq(hodei.catalog.outbox.key, {})) {{ {OUTBOX_FIELDS} }}\n \
         }}",
        dql_string(&repo),
        dql_string(&version),
        dql_string(&asset),
        dql_string(&asset),
        dql_string(event_id),
    );
    let response = client
        .execute_dql(&query)
        .await
        .map_err(|error| CatalogError::unavailable(error.to_string()))?;
    let node_of = |root: &str| -> Result<Option<serde_json::Value>, CatalogError> {
        let nodes = response
            .json
            .get(root)
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        match nodes.len() {
            0 => Ok(None),
            1 => Ok(Some(nodes[0].clone())),
            n => Err(CatalogError::corrupt(
                "row",
                root,
                format!("{n} rows carry the same key"),
            )),
        }
    };
    let repository = node_of("r")?
        .map(|node| RepositoryRow::from_node(&node))
        .transpose()?;
    let version = node_of("v")?
        .map(|node| VersionRow::from_node(&node))
        .transpose()?;
    let asset = node_of("a")?
        .map(|node| AssetRow::from_node(&node))
        .transpose()?;
    let reference = node_of("rf")?
        .map(|node| ReferenceRow::from_node(&node))
        .transpose()?;
    let event = node_of("ev")?
        .map(|node| OutboxRow::from_node(&node))
        .transpose()?;
    Ok(PublicationState {
        repository,
        version,
        asset,
        reference,
        event,
    })
}

/// Lo que la publicación tiene que hacer, decidido del estado: la misma
/// función antes de la transacción (para decidir) y después del commit
/// (para responder), porque la respuesta sale del estado y no de la
/// esperanza de que la transacción haya commiteado.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WriteKind {
    /// La ruta está vacía: crear asset, versión (si falta), referencia y
    /// evento.
    Create,
    /// La ruta apunta a otro contenido y la publicación lo sustituye: el
    /// puntero se mueve, la versión anterior no se toca.
    Replace {
        /// El digest que la ruta apunta ahora.
        previous: String,
    },
    /// La ruta ya apunta a este contenido: reparar lo que falte
    /// (referencia, versión) sin decir un hecho nuevo.
    Repair,
}

/// El plan decidido sobre un estado de publicación.
struct PublishPlan {
    /// Lo que hay que escribir, si hay que escribir.
    write: Option<WriteKind>,
    /// Todo lo que la publicación declara ya está: no se escribe nada ni
    /// se dice un hecho nuevo.
    complete: bool,
    /// Ya hay algo ahí con otro contenido.
    conflict: Option<CatalogError>,
    /// La condición que el cliente exigió no se cumple.
    precondition_failure: Option<CatalogError>,
}

impl PublishPlan {
    /// Decide el plan sobre el estado leído. La lógica es la del oráculo
    /// Surreal: el conflicto de versión decide primero (una versión
    /// publicada no cambia), después el de la ruta (una ruta que apunta a
    /// algo no se mueve sola), después la precondición, y solo entonces se
    /// planea la escritura. El repositorio que falta lo comprueba quien
    /// llama, antes de aquí.
    fn decide(state: &PublicationState, publication: &Publication) -> Self {
        // La versión es inmutable: si está con otro contenido, no se
        // escribe nada y el conflicto se dice.
        if let Some(version) = state.version.as_ref() {
            let same = version.digest == publication.digest
                && version.size_bytes == publication.size_bytes;
            if !same {
                return Self {
                    conflict: Some(CatalogError::Conflict {
                        kind: "version",
                        key: format!(
                            "{}/{}@{}",
                            publication.repository,
                            publication.coordinates.package_name,
                            publication.coordinates.package_version
                        ),
                        detail: "a published version does not change: publish another version"
                            .to_owned(),
                    }),
                    ..Self::empty()
                };
            }
        }

        match state.asset.as_ref() {
            None => {
                let precondition_failure = the_precondition_failure(publication, None);
                if let Some(failure) = precondition_failure {
                    return Self {
                        precondition_failure: Some(failure),
                        ..Self::empty()
                    };
                }
                Self {
                    write: Some(WriteKind::Create),
                    ..Self::empty()
                }
            }
            Some(asset) => {
                let same_digest = asset.digest == publication.digest
                    && asset.size_bytes == publication.size_bytes;
                if same_digest {
                    // La ruta ya apunta a este contenido: la publicación
                    // es lo mismo dicho dos veces. La precondición se
                    // comprueba contra lo que hay.
                    if let Some(failure) =
                        the_precondition_failure(publication, Some(&asset.digest))
                    {
                        return Self {
                            precondition_failure: Some(failure),
                            ..Self::empty()
                        };
                    }
                    if state.version.is_some() && state.reference.is_some() {
                        Self {
                            complete: true,
                            ..Self::empty()
                        }
                    } else {
                        Self {
                            write: Some(WriteKind::Repair),
                            ..Self::empty()
                        }
                    }
                } else if !publication.overwrite {
                    // La ruta ya apunta a otra cosa y nadie pidió
                    // sustituirla: el camino ya dice algo.
                    Self {
                        conflict: Some(CatalogError::Conflict {
                            kind: "asset",
                            key: format!("{}/{}", publication.repository, publication.asset_path),
                            detail: "the path already points at other content".to_owned(),
                        }),
                        ..Self::empty()
                    }
                } else if let Some(failure) =
                    the_precondition_failure(publication, Some(&asset.digest))
                {
                    Self {
                        precondition_failure: Some(failure),
                        ..Self::empty()
                    }
                } else {
                    Self {
                        write: Some(WriteKind::Replace {
                            previous: asset.digest.value.clone(),
                        }),
                        ..Self::empty()
                    }
                }
            }
        }
    }

    fn empty() -> Self {
        Self {
            write: None,
            complete: false,
            conflict: None,
            precondition_failure: None,
        }
    }
}

/// La condición que el cliente exigía, decidida contra el digest que hay
/// en la ruta (o nada, si la ruta está vacía).
fn the_precondition_failure(
    publication: &Publication,
    current: Option<&ContentDigest>,
) -> Option<CatalogError> {
    let key = format!("{}/{}", publication.repository, publication.asset_path);
    let failure = |detail: String| {
        Some(CatalogError::PreconditionFailed {
            kind: "asset",
            key: key.clone(),
            detail,
        })
    };
    match &publication.precondition {
        None => None,
        Some(Precondition::Absent) => {
            if current.is_none() {
                None
            } else {
                failure("the path must be empty, and it already points somewhere".to_owned())
            }
        }
        Some(Precondition::Exists) => {
            if current.is_some() {
                None
            } else {
                failure("the path must point somewhere, and it is empty".to_owned())
            }
        }
        Some(Precondition::Present(digests)) => {
            let Some(current) = current else {
                return failure(
                    "the path must point at one of the requested contents, and it is empty"
                        .to_owned(),
                );
            };
            if digests.iter().any(|digest| digest == current) {
                None
            } else {
                failure(
                    "the path points at content that is not one of the requested digests"
                        .to_owned(),
                )
            }
        }
        Some(Precondition::Other(digests)) => {
            let current = current?;
            if digests.iter().any(|digest| digest == current) {
                failure("the path points at one of the contents the write forbids".to_owned())
            } else {
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Los N-Quads de cada fila.
// ---------------------------------------------------------------------------

/// El nombre del ámbito, tal y como va a la base.
fn scope_name(scope: DigestScope) -> &'static str {
    match scope {
        DigestScope::Global => "global",
        DigestScope::AccountPartitioned => "account_partitioned",
    }
}

/// El resultado que la operación graba en su marcador: lo que un replay
/// devuelve como respuesta ya comprometida.
fn the_recorded_result(kind: &WriteKind) -> String {
    match kind {
        WriteKind::Create => "created".to_owned(),
        WriteKind::Replace { .. } => "replaced".to_owned(),
        WriteKind::Repair => "repaired".to_owned(),
    }
}

/// El evento de auditoría como fila del outbox, en la MISMA transacción
/// que las filas del catálogo.
fn outbox_event_nquads(event: &AuditEvent) -> String {
    format!(
        "_:evn <dgraph.type> \"{OUTBOX_TYPE}\" .\n\
         _:evn <hodei.catalog.outbox.key> {key} .\n\
         _:evn <hodei.catalog.outbox.tenant> {tenant} .\n\
         _:evn <hodei.catalog.outbox.actor> {actor} .\n\
         _:evn <hodei.catalog.outbox.correlation_id> {correlation} .\n\
         _:evn <hodei.catalog.outbox.action> {action} .\n\
         _:evn <hodei.catalog.outbox.resource> {resource} .\n\
         _:evn <hodei.catalog.outbox.digest> {digest} .\n\
         _:evn <hodei.catalog.outbox.result> \"{result}\" .\n\
         _:evn <hodei.catalog.outbox.occurred_at> \"{occurred}\" .\n\
         _:evn <hodei.catalog.outbox.state> \"pending\" .\n\
         _:evn <hodei.catalog.outbox.attempts> \"0\" .\n",
        key = dql_string(&event.event_id),
        tenant = dql_string(event.tenant()),
        actor = dql_string(event.actor()),
        correlation = dql_string(event.correlation_id()),
        action = dql_string(&event.action),
        resource = dql_string(&event.resource),
        digest = dql_string(event.digest.as_deref().unwrap_or_default()),
        result = event.result,
        occurred = event.timestamp.to_rfc3339_opts(SecondsFormat::Millis, true),
    )
}

/// La fila del asset que nace con la publicación.
fn asset_create_nquads(account: &str, publication: &Publication, now: &str) -> String {
    let subject = publication
        .subject
        .as_ref()
        .map(|digest| {
            format!(
                "_:an <hodei.catalog.asset.subject> {} .\n",
                dql_string(&digest.value)
            )
        })
        .unwrap_or_default();
    format!(
        "_:an <dgraph.type> \"{ASSET_TYPE}\" .\n\
         _:an <hodei.catalog.asset.key> {key} .\n\
         _:an <hodei.catalog.asset.account> {account} .\n\
         _:an <hodei.catalog.asset.repository> {repository} .\n\
         _:an <hodei.catalog.asset.asset_path> {path} .\n\
         _:an <hodei.catalog.asset.package_name> {name} .\n\
         _:an <hodei.catalog.asset.package_version> {version} .\n\
         _:an <hodei.catalog.asset.digest_algorithm> \"{algorithm}\" .\n\
         _:an <hodei.catalog.asset.digest> {digest} .\n\
         _:an <hodei.catalog.asset.size_bytes> \"{size}\" .\n\
         _:an <hodei.catalog.asset.updated_at> \"{now}\" .\n\
         {subject}",
        key = dql_string(&asset_key(
            account,
            &publication.repository,
            &publication.asset_path
        )),
        account = dql_string(account),
        repository = dql_string(&publication.repository),
        path = dql_string(&publication.asset_path),
        name = dql_string(&publication.coordinates.package_name),
        version = dql_string(&publication.coordinates.package_version),
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        size = publication.size_bytes,
    )
}

/// La fila de la versión que nace con la publicación.
fn version_create_nquads(account: &str, publication: &Publication, now: &str) -> String {
    format!(
        "_:vn <dgraph.type> \"{VERSION_TYPE}\" .\n\
         _:vn <hodei.catalog.version.key> {key} .\n\
         _:vn <hodei.catalog.version.account> {account} .\n\
         _:vn <hodei.catalog.version.repository> {repository} .\n\
         _:vn <hodei.catalog.version.package_name> {name} .\n\
         _:vn <hodei.catalog.version.package_version> {version} .\n\
         _:vn <hodei.catalog.version.digest_algorithm> \"{algorithm}\" .\n\
         _:vn <hodei.catalog.version.digest> {digest} .\n\
         _:vn <hodei.catalog.version.size_bytes> \"{size}\" .\n\
         _:vn <hodei.catalog.version.published_at> \"{now}\" .\n",
        key = dql_string(&version_key(
            account,
            &publication.repository,
            &publication.coordinates.package_name,
            &publication.coordinates.package_version,
        )),
        account = dql_string(account),
        repository = dql_string(&publication.repository),
        name = dql_string(&publication.coordinates.package_name),
        version = dql_string(&publication.coordinates.package_version),
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        size = publication.size_bytes,
        now = now,
    )
}

/// La fila de la referencia al blob que nace con la publicación. Los bytes
/// viven en el CAS; la referencia solo dice donde están.
fn reference_create_nquads(
    account: &str,
    publication: &Publication,
    scope: DigestScope,
    now: &str,
) -> String {
    format!(
        "_:rn <dgraph.type> \"{REFERENCE_TYPE}\" .\n\
         _:rn <hodei.catalog.reference.key> {key} .\n\
         _:rn <hodei.catalog.reference.account> {account} .\n\
         _:rn <hodei.catalog.reference.repository> {repository} .\n\
         _:rn <hodei.catalog.reference.asset_path> {path} .\n\
         _:rn <hodei.catalog.reference.digest_algorithm> \"{algorithm}\" .\n\
         _:rn <hodei.catalog.reference.digest> {digest} .\n\
         _:rn <hodei.catalog.reference.scope> \"{scope}\" .\n\
         _:rn <hodei.catalog.reference.size_bytes> \"{size}\" .\n\
         _:rn <hodei.catalog.reference.trust_state> \"quarantined\" .\n\
         _:rn <hodei.catalog.reference.updated_at> \"{now}\" .\n",
        key = dql_string(&asset_key(
            account,
            &publication.repository,
            &publication.asset_path
        )),
        account = dql_string(account),
        repository = dql_string(&publication.repository),
        path = dql_string(&publication.asset_path),
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        scope = scope_name(scope),
        size = publication.size_bytes,
    )
}

/// La fila de un repositorio que nace.
fn repository_create_nquads(descriptor: &RepositoryDescriptor, now: &str) -> String {
    format!(
        "_:rn <dgraph.type> \"{REPOSITORY_TYPE}\" .\n\
         _:rn <hodei.catalog.repository.key> {key} .\n\
         _:rn <hodei.catalog.repository.account> {account} .\n\
         _:rn <hodei.catalog.repository.repo_key> {repo_key} .\n\
         _:rn <hodei.catalog.repository.hrn> {hrn} .\n\
         _:rn <hodei.catalog.repository.format> \"{format}\" .\n\
         _:rn <hodei.catalog.repository.mode> \"{mode}\" .\n\
         _:rn <hodei.catalog.repository.config_version> \"{version}\" .\n\
         _:rn <hodei.catalog.repository.config_scope> \"{scope}\" .\n\
         _:rn <hodei.catalog.repository.config_settings> {settings} .\n\
         _:rn <hodei.catalog.repository.created_at> \"{now}\" .\n\
         _:rn <hodei.catalog.repository.updated_at> \"{now}\" .\n",
        key = dql_string(&repository_key(&descriptor.account, &descriptor.repo_key)),
        account = dql_string(&descriptor.account),
        repo_key = dql_string(&descriptor.repo_key),
        hrn = dql_string(&descriptor.hrn.to_string()),
        format = descriptor.format.as_str(),
        mode = descriptor.mode.as_str(),
        version = descriptor.config.version,
        scope = scope_name(descriptor.config.digest_scope),
        settings = dql_string(&descriptor.config.config_settings.to_string()),
        now = now,
    )
}

/// La puerta del padre de cada bloque: la misma que la guarda usa, para
/// que una escritura cuya pre-lectura quedó obsoleta no escriba nada.
fn parent_gate(current: u64) -> &'static str {
    if current == 0 {
        "eq(len(op), 0) AND eq(len(g), 0)"
    } else {
        "eq(len(op), 0) AND eq(len(gv), 1)"
    }
}

/// Las líneas de consulta de una publicación: ligan exactamente las
/// variables que los bloques usan, porque DQL rechaza las variables que
/// nadie nombra.
fn the_effect_query_lines(
    kind: &WriteKind,
    event: &AuditEvent,
    event_said: bool,
    account: &str,
    publication: &Publication,
) -> Vec<String> {
    let version = version_key(
        account,
        &publication.repository,
        &publication.coordinates.package_name,
        &publication.coordinates.package_version,
    );
    let asset = asset_key(account, &publication.repository, &publication.asset_path);
    // DQL rechaza las variables definidas y no usadas: cada caso liga
    // exactamente lo que sus bloques nombran.
    let mut lines = vec![format!(
        "v as var(func: eq(hodei.catalog.version.key, {}))",
        dql_string(&version)
    )];
    match kind {
        WriteKind::Create => {
            lines.push(format!(
                "a as var(func: eq(hodei.catalog.asset.key, {}))",
                dql_string(&asset)
            ));
            lines.push(format!(
                "rf as var(func: eq(hodei.catalog.reference.key, {}))",
                dql_string(&asset)
            ));
            if !event_said {
                lines.push(format!(
                    "ev as var(func: eq(hodei.catalog.outbox.key, {}))",
                    dql_string(&event.event_id)
                ));
            }
        }
        WriteKind::Replace { previous } => {
            lines.push(format!(
                "am as var(func: eq(hodei.catalog.asset.key, {})) @filter(NOT eq(hodei.catalog.asset.digest, {}))",
                dql_string(&asset),
                dql_string(previous),
            ));
            lines.push(format!(
                "rf as var(func: eq(hodei.catalog.reference.key, {}))",
                dql_string(&asset)
            ));
            if !event_said {
                lines.push(format!(
                    "ev as var(func: eq(hodei.catalog.outbox.key, {}))",
                    dql_string(&event.event_id)
                ));
            }
        }
        WriteKind::Repair => {
            lines.push(format!(
                "rf as var(func: eq(hodei.catalog.reference.key, {}))",
                dql_string(&asset)
            ));
        }
    }
    lines
}

/// El movimiento de la fila del asset: el puntero pasa a apuntar al
/// contenido nuevo, por el uid que el filtro de la consulta ligó.
fn asset_move_nquads(publication: &Publication, now: &str) -> String {
    let subject = publication
        .subject
        .as_ref()
        .map(|digest| {
            format!(
                "uid(am) <hodei.catalog.asset.subject> {} .\n",
                dql_string(&digest.value)
            )
        })
        .unwrap_or_default();
    format!(
        "uid(am) <hodei.catalog.asset.digest_algorithm> \"{algorithm}\" .\n\
         uid(am) <hodei.catalog.asset.digest> {digest} .\n\
         uid(am) <hodei.catalog.asset.size_bytes> \"{size}\" .\n\
         uid(am) <hodei.catalog.asset.package_name> {name} .\n\
         uid(am) <hodei.catalog.asset.package_version> {version} .\n\
         uid(am) <hodei.catalog.asset.updated_at> \"{now}\" .\n\
         {subject}",
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        size = publication.size_bytes,
        name = dql_string(&publication.coordinates.package_name),
        version = dql_string(&publication.coordinates.package_version),
    )
}

/// El movimiento de la fila de la referencia, que se mueve con el asset.
fn reference_move_nquads(publication: &Publication, scope: DigestScope, now: &str) -> String {
    format!(
        "uid(rf) <hodei.catalog.reference.digest_algorithm> \"{algorithm}\" .\n\
         uid(rf) <hodei.catalog.reference.digest> {digest} .\n\
         uid(rf) <hodei.catalog.reference.scope> \"{scope}\" .\n\
         uid(rf) <hodei.catalog.reference.size_bytes> \"{size}\" .\n\
         uid(rf) <hodei.catalog.reference.updated_at> \"{now}\" .\n",
        algorithm = publication.digest.algorithm,
        digest = dql_string(&publication.digest.value),
        scope = scope_name(scope),
        size = publication.size_bytes,
    )
}

/// Los bloques condicionados de una publicación, con la puerta del padre
/// de la llamada. El contrato de la frontera: el evento se escribe solo
/// si las filas de contenido se escriben, y las filas de contenido solo
/// se escriben si el evento puede escribirse — un asset sin su evento es
/// un hecho sin rastro, y el gate lo impide antes de escribir nada.
#[allow(clippy::too_many_arguments)]
fn the_blocks(
    parent: &str,
    kind: &WriteKind,
    event: &AuditEvent,
    event_said: bool,
    account: &str,
    publication: &Publication,
    scope: DigestScope,
    now: &str,
) -> Vec<GraphMutationBlock> {
    // El gate del evento: mientras el hecho no esté dicho, las filas de
    // contenido llevan la condición de que la fila del evento esté libre.
    let ev_gate = if event_said {
        String::new()
    } else {
        " AND eq(len(ev), 0)".to_owned()
    };
    let mut blocks = Vec::new();
    match kind {
        WriteKind::Create => {
            if !event_said {
                blocks.push(GraphMutationBlock::set_if(
                    format!("{parent} AND eq(len(a), 0){ev_gate}"),
                    outbox_event_nquads(event),
                ));
            }
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(a), 0){ev_gate}"),
                asset_create_nquads(account, publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(a), 0) AND eq(len(v), 0){ev_gate}"),
                version_create_nquads(account, publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(a), 0) AND eq(len(rf), 0){ev_gate}"),
                reference_create_nquads(account, publication, scope, now),
            ));
        }
        WriteKind::Replace { .. } => {
            if !event_said {
                blocks.push(GraphMutationBlock::set_if(
                    format!("{parent} AND eq(len(am), 1){ev_gate}"),
                    outbox_event_nquads(event),
                ));
            }
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(am), 1){ev_gate}"),
                asset_move_nquads(publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(am), 1) AND eq(len(v), 0){ev_gate}"),
                version_create_nquads(account, publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(am), 1) AND eq(len(rf), 1){ev_gate}"),
                reference_move_nquads(publication, scope, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(am), 1) AND eq(len(rf), 0){ev_gate}"),
                reference_create_nquads(account, publication, scope, now),
            ));
        }
        WriteKind::Repair => {
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(v), 0)"),
                version_create_nquads(account, publication, now),
            ));
            blocks.push(GraphMutationBlock::set_if(
                format!("{parent} AND eq(len(rf), 0)"),
                reference_create_nquads(account, publication, scope, now),
            ));
        }
    }
    blocks
}

// ---------------------------------------------------------------------------
// El almacén.
// ---------------------------------------------------------------------------

/// El catálogo sobre Dgraph. Se abre con la guarda validada (el arranque
/// versionado del grafo) y declara su esquema aditivo propio encima.
pub struct DgraphCatalogStore {
    client: Arc<GraphClient>,
    guards: GuardStore,
    authorizer: Arc<dyn CatalogAuthorizer>,
    probe: Option<Arc<dyn BlobIntegrityProbe>>,
    identity: Identity,
}

impl std::fmt::Debug for DgraphCatalogStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DgraphCatalogStore").finish_non_exhaustive()
    }
}

/// La decisión de un guard error tipado del cliente, dicho como error del
/// catálogo. Nada se calla: cada clase se dice como es.
fn the_guard_error(error: hodei_graph_client::GuardError) -> CatalogError {
    match error {
        hodei_graph_client::GuardError::InvalidScope { scope, reason } => {
            CatalogError::invalid(format!("the scope '{scope}' is not valid: {reason}"))
        }
        hodei_graph_client::GuardError::Operation(_) => {
            CatalogError::unavailable(error.to_string())
        }
        hodei_graph_client::GuardError::CasRejected { .. } => CatalogError::Conflict {
            kind: "asset",
            key: "guard".to_owned(),
            detail: error.to_string(),
        },
        hodei_graph_client::GuardError::CorruptState { .. } => {
            CatalogError::corrupt("row", "guard", error.to_string())
        }
        _ => CatalogError::unavailable(error.to_string()),
    }
}

impl DgraphCatalogStore {
    /// Abre el almacén sobre un cliente ya conectado y autenticado: la
    /// guarda valida el esquema versionado del grafo (v7) y después este
    /// slice declara su DDL aditivo propio, que es idempotente.
    pub async fn open(
        client: Arc<GraphClient>,
        authorizer: Arc<dyn CatalogAuthorizer>,
    ) -> Result<Self, CatalogError> {
        let guards = GuardStore::open(client.clone())
            .await
            .map_err(the_guard_error)?;
        client
            .alter_schema(&catalog_schema_dql())
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        Ok(Self {
            client,
            guards,
            authorizer,
            probe: None,
            identity: Identity::system(),
        })
    }

    /// El almacén con el decisor que la frontera ya no repite.
    pub fn with_authorizer(mut self, authorizer: Arc<dyn CatalogAuthorizer>) -> Self {
        self.authorizer = authorizer;
        self
    }

    /// El almacén con la sonda de integridad del CAS.
    pub fn with_probe(mut self, probe: Arc<dyn BlobIntegrityProbe>) -> Self {
        self.probe = Some(probe);
        self
    }

    /// El almacén con la identidad (reloj e identificadores) que el rastro
    /// usa.
    pub fn with_identity(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    /// Decide la petición contra el autorizador inyectado.
    async fn decide(&self, request: &AccessRequest) -> Result<(), CatalogError> {
        match self.authorizer.authorize(request).await? {
            AccessDecision::Allow => Ok(()),
            AccessDecision::Deny(reason) => Err(CatalogError::Denied {
                action: request.action.as_str(),
                kind: CatalogResource::as_str(&request.resource),
                key: request.subject.clone(),
                reason,
            }),
        }
    }

    /// La fila de un repositorio por su clave, sin autorizar.
    async fn the_repository_row(
        &self,
        account: &str,
        repo_key: &str,
    ) -> Result<Option<RepositoryRow>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.repository.key, {})) {{ {REPOSITORY_FIELDS} }} }}",
            dql_string(&repository_key(account, repo_key)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        single_row(
            response.json.get("q"),
            "repository",
            format!("{account}/{repo_key}"),
        )
        .map(|node| node.map(|node| RepositoryRow::from_node(&node)).transpose())
        .and_then(|row| row)
    }

    /// La fila de un asset por su clave, sin autorizar.
    async fn the_asset_row(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<Option<AssetRow>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.asset.key, {})) {{ {ASSET_FIELDS} }} }}",
            dql_string(&asset_key(account, repository, asset_path)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        single_row(
            response.json.get("q"),
            "asset",
            format!("{repository}/{asset_path}"),
        )
        .map(|node| node.map(|node| AssetRow::from_node(&node)).transpose())
        .and_then(|row| row)
    }

    /// La fila de una versión por sus coordenadas, sin autorizar.
    async fn the_version_row(
        &self,
        account: &str,
        repository: &str,
        coordinates: &PackageCoordinates,
    ) -> Result<Option<VersionRow>, CatalogError> {
        let key = version_key(
            account,
            repository,
            &coordinates.package_name,
            &coordinates.package_version,
        );
        let query = format!(
            "{{ q(func: eq(hodei.catalog.version.key, {})) {{ {VERSION_FIELDS} }} }}",
            dql_string(&key),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let kind = format!(
            "{repository}/{}@{}",
            coordinates.package_name, coordinates.package_version
        );
        single_row(response.json.get("q"), "version", kind)
            .map(|node| node.map(|node| VersionRow::from_node(&node)).transpose())
            .and_then(|row| row)
    }

    /// La fila de una referencia por su clave, sin autorizar.
    async fn the_reference_row(
        &self,
        account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<Option<ReferenceRow>, CatalogError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.reference.key, {})) {{ {REFERENCE_FIELDS} }} }}",
            dql_string(&asset_key(account, repository, asset_path)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        single_row(
            response.json.get("q"),
            "reference",
            format!("{repository}/{asset_path}"),
        )
        .map(|node| node.map(|node| ReferenceRow::from_node(&node)).transpose())
        .and_then(|row| row)
    }

    /// El evento de auditoría de una publicación. El **tenant** es la
    /// cuenta verificada del principal, no la que dice la publicación.
    fn the_publication_event(
        &self,
        event_id: &str,
        occurred_at: DateTime<Utc>,
        principal_account: &str,
        publication: &Publication,
        result: AuditResult,
    ) -> AuditEvent {
        AuditEvent::new(
            event_id.to_string(),
            AuditContext::new(
                publication.actor.clone(),
                principal_account.to_string(),
                publication.correlation_id.clone(),
            ),
            occurred_at,
            "publish",
            format!("{}/{}", publication.repository, publication.asset_path),
            Some(format!(
                "{}:{}",
                publication.digest.algorithm, publication.digest.value
            )),
            result,
        )
    }

    /// Dice el evento de una publicación que no llegó a publicarse, fuera
    /// de la transacción del catálogo: un rechazo o un fallo no escriben
    /// filas del catálogo y no hay mutación que atar. Si la escritura
    /// falla, se avisa y no se finge que el rastro está.
    async fn audit_the_publication(
        &self,
        event_id: &str,
        occurred_at: DateTime<Utc>,
        principal_account: &str,
        publication: &Publication,
        result: AuditResult,
    ) {
        let event = self.the_publication_event(
            event_id,
            occurred_at,
            principal_account,
            publication,
            result,
        );
        let outbox = DgraphAuditOutbox::new(self.client.clone());
        if let Err(error) = outbox.put_pending(&event).await {
            tracing::warn!(
                event_id = %event.event_id,
                error = %error,
                "the audit event could not be written to the outbox"
            );
        }
    }

    /// Lo que puede fallar antes de escribir, por el contenido: el CAS
    /// tiene que tener los bytes que la publicación declara. Sin sonda no
    /// se puede comprobar, y el catálogo guarda lo que se le declara.
    async fn the_probe_failure(
        &self,
        principal_account: &str,
        scope: DigestScope,
        publication: &Publication,
    ) -> Option<CatalogError> {
        let probe = self.probe.as_ref()?;
        let location = BlobLocation::new(principal_account, scope, publication.digest.clone());
        match probe.length_of(&location).await {
            Ok(size) if size == publication.size_bytes => None,
            Ok(size) => Some(CatalogError::invalid(format!(
                "the content in the store has {size} bytes and the publication declares {}",
                publication.size_bytes
            ))),
            Err(BlobProbeError::Missing) => Some(CatalogError::invalid(
                "the content is not in the blob store",
            )),
            Err(BlobProbeError::Unavailable { detail }) => Some(CatalogError::unavailable(detail)),
        }
    }

    /// Escribe una publicación: la guarda de la ruta avanza y, en la MISMA
    /// transacción, las filas del catálogo y el evento de auditoría se
    /// aplican o no se aplica ninguno. Devuelve lo que el marcador de la
    /// operación contestó.
    #[allow(clippy::too_many_arguments)]
    async fn write_the_publication(
        &self,
        guard: &GuardScope,
        kind: &WriteKind,
        event: &AuditEvent,
        event_said: bool,
        principal_account: &str,
        publication: &Publication,
        scope: DigestScope,
    ) -> Result<hodei_graph_client::GuardedOutcome, CatalogError> {
        let operation = publication_operation_id(principal_account, publication);
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let effect = GuardedEffect {
            query_lines: the_effect_query_lines(
                kind,
                event,
                event_said,
                principal_account,
                publication,
            ),
            set_nquads: String::new(),
            recorded_result: the_recorded_result(kind),
        };
        self.guards
            .execute_guarded_composite(
                guard,
                &operation,
                RevisionDemand::Advance,
                effect,
                &|current| {
                    the_blocks(
                        parent_gate(current),
                        kind,
                        event,
                        event_said,
                        principal_account,
                        publication,
                        scope,
                        &now,
                    )
                },
            )
            .await
            .map_err(the_guard_error)
    }
}

/// La fila única que una lectura por clave puede devolver: cero, una, o
/// corrupción (dos filas con la misma clave no son dos respuestas).
fn single_row(
    nodes: Option<&serde_json::Value>,
    kind: &'static str,
    key: String,
) -> Result<Option<serde_json::Value>, CatalogError> {
    let nodes = nodes
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    match nodes.len() {
        0 => Ok(None),
        1 => Ok(Some(nodes[0].clone())),
        n => Err(CatalogError::corrupt(
            kind,
            key,
            format!("{n} rows carry the same key"),
        )),
    }
}

/// Lo que la publicación dejó, construido del estado que el grafo dice
/// ahora. La respuesta sale del estado, nunca de la esperanza.
fn the_outcome_from_state(
    state: &PublicationState,
    principal_account: &str,
    publication: &Publication,
    created: bool,
    disposition: AssetDisposition,
) -> Result<PublishOutcome, CatalogError> {
    let key = format!("{}/{}", publication.repository, publication.asset_path);
    let asset_row = state.asset.as_ref().ok_or_else(|| {
        CatalogError::unavailable(format!("the asset {key} is not there after the commit"))
    })?;
    let asset = asset_row.clone().into_record(
        principal_account,
        &publication.repository,
        &publication.asset_path,
    )?;
    let version = state
        .version
        .as_ref()
        .map(|row| row.clone().into_package_version());
    let reference_row = state.reference.as_ref().ok_or_else(|| {
        CatalogError::unavailable(format!("the reference {key} is not there after the commit"))
    })?;
    let reference = reference_row.clone().into_reference(
        principal_account.to_string(),
        publication.repository.clone(),
        publication.asset_path.clone(),
    );
    Ok(PublishOutcome {
        version: version.unwrap_or(PackageVersion {
            account: principal_account.to_string(),
            repository: publication.repository.clone(),
            coordinates: publication.coordinates.clone(),
            digest: publication.digest.clone(),
            size_bytes: publication.size_bytes,
            published_at: Utc::now(),
        }),
        asset,
        reference,
        created,
        disposition,
    })
}

#[async_trait]
impl CatalogReader for DgraphCatalogStore {
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
        self.the_repository_row(principal_account, repo_key)
            .await?
            .ok_or_else(|| CatalogError::missing("repository", repo_key.to_string()))?
            .into_descriptor(principal_account, repo_key)
    }

    async fn assets_under(
        &self,
        principal_account: &str,
        repository: &str,
        prefix: &str,
    ) -> Result<Vec<AssetRecord>, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Read,
            repository,
        ))
        .await?;
        // El rango del prefijo sobre el índice exacto del camino: todo lo
        // que empieza por el prefijo es mayor o igual que el prefijo y
        // menor que el prefijo con el último carácter subido.
        let upper = format!("{prefix}\u{10FFFF}");
        let query = format!(
            "{{ q(func: eq(hodei.catalog.asset.repository, {}), orderasc: hodei.catalog.asset.asset_path, first: {}) \
             @filter(eq(hodei.catalog.asset.account, {}) AND ge(hodei.catalog.asset.asset_path, {}) AND le(hodei.catalog.asset.asset_path, {})) \
             {{ {ASSET_FIELDS} }} }}",
            dql_string(repository),
            hodei_catalog::MAX_ASSETS_UNDER + 1,
            dql_string(principal_account),
            dql_string(prefix),
            dql_string(&upper),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        if nodes.len() > hodei_catalog::MAX_ASSETS_UNDER {
            return Err(CatalogError::invalid(
                "the prefix holds more assets than the catalog is willing to list",
            ));
        }
        nodes
            .iter()
            .map(|node| {
                let row = AssetRow::from_node(node)?;
                let path = row.asset_path.clone();
                row.into_record(principal_account, repository, &path)
            })
            .collect()
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
            .await?
            .ok_or_else(|| CatalogError::missing("asset", key.clone()))?;

        // La versión a la que apunta tiene que decir el mismo contenido.
        let coordinates = PackageCoordinates {
            package_name: asset.package_name.clone(),
            package_version: asset.package_version.clone(),
        };
        let version = self
            .the_version_row(principal_account, repository, &coordinates)
            .await?
            .ok_or_else(|| {
                CatalogError::corrupt(
                    "asset",
                    key.clone(),
                    "the asset points at a version that is not there",
                )
            })?;
        if version.digest != asset.digest {
            return Err(CatalogError::corrupt(
                "asset",
                key.clone(),
                "the asset and its version point at different content",
            ));
        }
        if version.size_bytes != asset.size_bytes {
            return Err(CatalogError::corrupt(
                "asset",
                key.clone(),
                "the asset and its version disagree on the size",
            ));
        }

        // Y la referencia tiene que apuntar al mismo contenido que el
        // asset: si no, la descarga serviría un blob y la recolección
        // guardaría otro.
        let reference = self
            .the_reference_row(principal_account, repository, asset_path)
            .await?
            .ok_or_else(|| {
                CatalogError::corrupt("asset", key.clone(), "the asset has no reference to a blob")
            })?;
        if reference.digest != asset.digest {
            return Err(CatalogError::corrupt(
                "asset",
                key.clone(),
                "the asset and its blob reference point at different content",
            ));
        }

        // Y el blob del CAS tiene que ser del tamaño que el catálogo dice.
        if let Some(probe) = &self.probe {
            let location =
                BlobLocation::new(principal_account, reference.scope, asset.digest.clone());
            match probe.length_of(&location).await {
                Ok(size) if size == asset.size_bytes => {}
                Ok(size) => {
                    return Err(CatalogError::corrupt(
                        "asset",
                        key,
                        format!(
                            "the blob has {size} bytes and the catalog says {}",
                            asset.size_bytes
                        ),
                    ));
                }
                Err(BlobProbeError::Missing) => {
                    return Err(CatalogError::corrupt(
                        "asset",
                        key,
                        "the blob is not in the store",
                    ));
                }
                Err(BlobProbeError::Unavailable { detail }) => {
                    return Err(CatalogError::unavailable(detail));
                }
            }
        }
        asset.into_record(principal_account, repository, asset_path)
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
        self.the_version_row(principal_account, repository, coordinates)
            .await?
            .ok_or_else(|| {
                CatalogError::missing(
                    "version",
                    format!(
                        "{repository}/{}@{}",
                        coordinates.package_name, coordinates.package_version
                    ),
                )
            })
            .map(|row| row.into_package_version())
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
        Err(CatalogError::missing("tag", tag_name.to_string()))
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
        self.the_reference_row(principal_account, repository, asset_path)
            .await?
            .ok_or_else(|| CatalogError::missing("reference", format!("{repository}/{asset_path}")))
            .map(|row| {
                row.into_reference(
                    principal_account.to_string(),
                    repository.to_string(),
                    asset_path.to_string(),
                )
            })
    }

    async fn tags(
        &self,
        principal_account: &str,
        repository: &str,
    ) -> Result<Vec<MutableTag>, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Read,
            repository,
        ))
        .await?;
        // El slice Raw no sirve etiquetas mutables: la lista honesta es la
        // vacía, no una etiqueta que no existe.
        Ok(Vec::new())
    }

    async fn referrers(
        &self,
        principal_account: &str,
        repository: &str,
        subject: &ContentDigest,
    ) -> Result<Vec<ReferrerRecord>, CatalogError> {
        self.decide(&AccessRequest::repository(
            principal_account,
            CatalogAction::Read,
            repository,
        ))
        .await?;
        let query = format!(
            "{{ q(func: eq(hodei.catalog.asset.subject, {})) \
             @filter(eq(hodei.catalog.asset.account, {}) AND eq(hodei.catalog.asset.repository, {})) \
             {{ {ASSET_FIELDS} }} }}",
            dql_string(&subject.value),
            dql_string(principal_account),
            dql_string(repository),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| CatalogError::unavailable(error.to_string()))?;
        let nodes = response
            .json
            .get("q")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut referrers = Vec::new();
        for node in &nodes {
            let row = AssetRow::from_node(node)?;
            referrers.push(ReferrerRecord {
                account: row.account,
                repository: row.repository,
                asset_path: row.asset_path,
                digest: row.digest,
                size_bytes: row.size_bytes,
                subject: ContentDigest {
                    algorithm: subject.algorithm.clone(),
                    value: node_text(node, "hodei.catalog.asset.subject")?,
                },
            });
        }
        Ok(referrers)
    }
}

#[async_trait]
impl CatalogWriter for DgraphCatalogStore {
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
        if descriptor.account != principal_account {
            return Err(CatalogError::invalid(
                "the repository belongs to another account",
            ));
        }
        let guard = repository_guard_scope(principal_account, &descriptor.repo_key)?;
        let operation = repository_operation_id(&descriptor);
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let key = repository_key(principal_account, &descriptor.repo_key);

        for _attempt in 0..WRITE_ATTEMPTS {
            // Lo que hay decide: la misma creación dichas dos veces es el
            // mismo repositorio; la clave ocupada con otro contenido es un
            // conflicto que el índice arbitra.
            if let Some(row) = self
                .the_repository_row(principal_account, &descriptor.repo_key)
                .await?
            {
                let existing = row.into_descriptor(principal_account, &descriptor.repo_key)?;
                let same = existing.format == descriptor.format
                    && existing.mode == descriptor.mode
                    && existing.config.digest_scope == descriptor.config.digest_scope
                    && existing.config.config_settings == descriptor.config.config_settings;
                if same {
                    return Ok(existing);
                }
                return Err(CatalogError::Conflict {
                    kind: "repository",
                    key: descriptor.repo_key.clone(),
                    detail: "the key is taken with different content".to_owned(),
                });
            }
            let effect = GuardedEffect {
                query_lines: vec![format!(
                    "r as var(func: eq(hodei.catalog.repository.key, {}))",
                    dql_string(&key)
                )],
                set_nquads: String::new(),
                recorded_result: "created".to_owned(),
            };
            self.guards
                .execute_guarded_composite(
                    &guard,
                    &operation,
                    RevisionDemand::Advance,
                    effect,
                    &|current| {
                        vec![GraphMutationBlock::set_if(
                            format!("{} AND eq(len(r), 0)", parent_gate(current)),
                            repository_create_nquads(&descriptor, &now),
                        )]
                    },
                )
                .await
                .map_err(the_guard_error)?;
            if let Some(row) = self
                .the_repository_row(principal_account, &descriptor.repo_key)
                .await?
            {
                return row.into_descriptor(principal_account, &descriptor.repo_key);
            }
            // La fila no quedó: otro escritor ganó la ventana. La
            // siguiente intenta relee y decide con lo que hay.
        }
        Err(CatalogError::unavailable(
            "the repository could not be created after the attempts",
        ))
    }

    async fn update_config(
        &self,
        _principal_account: &str,
        _repo_key: &str,
        _config: RepositoryConfig,
    ) -> Result<RepositoryDescriptor, CatalogError> {
        // El slice Raw no sirve reconfiguraciones: lo dice, no lo finge.
        Err(CatalogError::invalid(
            "the dgraph raw slice does not serve configuration changes",
        ))
    }

    async fn publish(
        &self,
        principal_account: &str,
        publication: Publication,
    ) -> Result<PublishOutcome, CatalogError> {
        // El evento de auditoría se identifica una vez por operación: el
        // mismo hecho no se dice dos veces con dos nombres, ni se dice con
        // dos resultados distintos.
        let event_id = self.identity.new_id();
        let occurred_at = self.identity.now();

        // Se decide **antes** de validar: un principal sin permiso no
        // llega a saber si lo que pide está bien formado.
        if let Err(error) = self
            .decide(&AccessRequest::publish(
                principal_account,
                &publication.repository,
                &publication.asset_path,
            ))
            .await
        {
            let result = if matches!(error, CatalogError::Denied { .. }) {
                AuditResult::Denied
            } else {
                AuditResult::Failed
            };
            self.audit_the_publication(
                &event_id,
                occurred_at,
                principal_account,
                &publication,
                result,
            )
            .await;
            return Err(error);
        }

        // Una petición **mal formada** se rechaza antes de la frontera de
        // efecto y no deja rastro: no hubo una operación que auditar.
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
        let guard = asset_guard_scope(
            principal_account,
            &publication.repository,
            &publication.asset_path,
        )?;

        let mut last_failure = String::new();
        for _attempt in 0..WRITE_ATTEMPTS {
            let state =
                read_publication_state(&self.client, principal_account, &publication, &event_id)
                    .await?;

            // El identificador del evento ya está dicho con OTRO hecho: la
            // publicación no puede salir, y la contradicción se reporta,
            // no se calla.
            if let Some(row) = state.event.as_ref() {
                let planned = self.the_publication_event(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Allowed,
                );
                if !row.the_event().same_fact(&planned) {
                    return Err(CatalogError::unavailable(format!(
                        "the audit event {event_id} is already said with another content"
                    )));
                }
            }

            let Some(repository_row) = state.repository.as_ref() else {
                // El repositorio no existe. La petición ocurrió y el
                // sistema la rechazó, y eso se dice.
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(CatalogError::missing(
                    "repository",
                    publication.repository.clone(),
                ));
            };
            if repository_row.account != principal_account
                || repository_row.repo_key != publication.repository
            {
                return Err(CatalogError::corrupt(
                    "repository",
                    publication.repository.clone(),
                    "the row does not carry the identity its key names",
                ));
            }
            let mode = RepositoryMode::parse(&repository_row.mode).ok_or_else(|| {
                CatalogError::corrupt(
                    "repository",
                    publication.repository.clone(),
                    "the mode is not a mode",
                )
            })?;
            if !mode.stores_content() {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(CatalogError::ModeNotImplemented { mode });
            }
            let scope = match repository_row.config_scope.as_str() {
                "global" => DigestScope::Global,
                "account_partitioned" => DigestScope::AccountPartitioned,
                other => {
                    return Err(CatalogError::corrupt(
                        "repository",
                        publication.repository.clone(),
                        format!("'{other}' is not a digest scope"),
                    ));
                }
            };
            if let Some(failure) = self
                .the_probe_failure(principal_account, scope, &publication)
                .await
            {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(failure);
            }

            let plan = PublishPlan::decide(&state, &publication);
            if let Some(conflict) = plan.conflict {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Conflict,
                )
                .await;
                return Err(conflict);
            }
            if let Some(failure) = plan.precondition_failure {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(failure);
            }
            if plan.complete {
                // No se escribe nada: no hay mutación, y un evento que
                // dice "publicado" sobre una publicación que ya estaba no
                // añade ningún hecho nuevo.
                return the_outcome_from_state(
                    &state,
                    principal_account,
                    &publication,
                    false,
                    AssetDisposition::Unchanged,
                );
            }

            let kind = plan.write.clone().expect("the plan demands a write");
            let event = self.the_publication_event(
                &event_id,
                occurred_at,
                principal_account,
                &publication,
                AuditResult::Allowed,
            );
            let event_said = state.event.is_some();
            let outcome = self
                .write_the_publication(
                    &guard,
                    &kind,
                    &event,
                    event_said,
                    principal_account,
                    &publication,
                    scope,
                )
                .await?;
            if outcome.replayed {
                // El mismo hecho ya está comprometido: el reintento tras el
                // commit relee el resultado comprometido y no escribe nada.
                let state = read_publication_state(
                    &self.client,
                    principal_account,
                    &publication,
                    &event_id,
                )
                .await?;
                let plan = PublishPlan::decide(&state, &publication);
                if plan.complete {
                    return the_outcome_from_state(
                        &state,
                        principal_account,
                        &publication,
                        false,
                        AssetDisposition::Unchanged,
                    );
                }
                last_failure =
                    "the operation replayed but the state does not say the fact".to_owned();
                continue;
            }
            // El commit contestó; el ESTADO decide lo que esta llamada
            // dejó: una creación que se asentó es una creación, y una
            // sustitución que se asentó es una sustitución.
            let state =
                read_publication_state(&self.client, principal_account, &publication, &event_id)
                    .await?;
            let plan = PublishPlan::decide(&state, &publication);
            if let Some(conflict) = plan.conflict {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Conflict,
                )
                .await;
                return Err(conflict);
            }
            if let Some(failure) = plan.precondition_failure {
                self.audit_the_publication(
                    &event_id,
                    occurred_at,
                    principal_account,
                    &publication,
                    AuditResult::Failed,
                )
                .await;
                return Err(failure);
            }
            if plan.complete {
                let created = !matches!(kind, WriteKind::Repair);
                let disposition = match &kind {
                    WriteKind::Create if created => AssetDisposition::Created,
                    WriteKind::Replace { .. } if created => AssetDisposition::Replaced,
                    _ => AssetDisposition::Unchanged,
                };
                return the_outcome_from_state(
                    &state,
                    principal_account,
                    &publication,
                    created,
                    disposition,
                );
            }
            last_failure = "the write did not settle after the commit".to_owned();
        }

        // El último intento también se relee: perder la carrera en el
        // último intento es un conflicto, no un fallo del almacenamiento.
        let state =
            read_publication_state(&self.client, principal_account, &publication, &event_id)
                .await?;
        let plan = PublishPlan::decide(&state, &publication);
        if let Some(conflict) = plan.conflict {
            self.audit_the_publication(
                &event_id,
                occurred_at,
                principal_account,
                &publication,
                AuditResult::Conflict,
            )
            .await;
            return Err(conflict);
        }
        if let Some(failure) = plan.precondition_failure {
            self.audit_the_publication(
                &event_id,
                occurred_at,
                principal_account,
                &publication,
                AuditResult::Failed,
            )
            .await;
            return Err(failure);
        }
        if plan.complete {
            return the_outcome_from_state(
                &state,
                principal_account,
                &publication,
                false,
                AssetDisposition::Unchanged,
            );
        }
        self.audit_the_publication(
            &event_id,
            occurred_at,
            principal_account,
            &publication,
            AuditResult::Failed,
        )
        .await;
        Err(CatalogError::unavailable(last_failure))
    }

    async fn move_tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
        _coordinates: PackageCoordinates,
    ) -> Result<MutableTag, CatalogError> {
        self.decide(&AccessRequest::tag(
            principal_account,
            CatalogAction::MoveTag,
            repository,
            tag_name,
        ))
        .await?;
        Err(CatalogError::invalid(
            "the dgraph raw slice does not serve mutable tags",
        ))
    }

    async fn move_asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
        _coordinates: PackageCoordinates,
    ) -> Result<AssetRecord, CatalogError> {
        self.decide(&AccessRequest::asset(
            principal_account,
            CatalogAction::MoveAsset,
            repository,
            asset_path,
        ))
        .await?;
        Err(CatalogError::invalid(
            "the dgraph raw slice does not serve asset moves",
        ))
    }
}

// ---------------------------------------------------------------------------
// El outbox de auditoría sobre el grafo.
// ---------------------------------------------------------------------------

/// La fila de un evento tal como está en el grafo.
#[derive(Debug, Clone)]
struct OutboxRow {
    event_id: String,
    tenant: String,
    actor: String,
    correlation_id: String,
    action: String,
    resource: String,
    digest: Option<String>,
    result: String,
    occurred_at: DateTime<Utc>,
    state: String,
    attempts: u64,
    lease_owner: Option<String>,
    lease_until: Option<DateTime<Utc>>,
    last_error: Option<String>,
    delivered_at: Option<DateTime<Utc>>,
}

/// El resultado de un evento, dicho como texto.
fn the_result(value: &str) -> AuditResult {
    match value {
        "allowed" => AuditResult::Allowed,
        "denied" => AuditResult::Denied,
        "conflict" => AuditResult::Conflict,
        _ => AuditResult::Failed,
    }
}

/// El estado de una entrada, dicho como texto.
fn the_state(value: &str) -> EntryState {
    match value {
        "claimed" => EntryState::Claimed,
        "delivered" => EntryState::Delivered,
        _ => EntryState::Pending,
    }
}

/// Un texto opcional de un nodo.
fn node_option_text(node: &serde_json::Value, field: &str) -> Option<String> {
    node.get(field)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// Un momento opcional de un nodo.
fn node_option_time(node: &serde_json::Value, field: &str) -> Option<DateTime<Utc>> {
    node_option_text(node, field).and_then(|text| {
        DateTime::parse_from_rfc3339(&text)
            .ok()
            .map(|instant| instant.with_timezone(&Utc))
    })
}

impl OutboxRow {
    fn from_node(node: &serde_json::Value) -> Result<Self, CatalogError> {
        Ok(Self {
            event_id: node_text(node, "hodei.catalog.outbox.key")?,
            tenant: node_text(node, "hodei.catalog.outbox.tenant")?,
            actor: node_text(node, "hodei.catalog.outbox.actor")?,
            correlation_id: node_text(node, "hodei.catalog.outbox.correlation_id")?,
            action: node_text(node, "hodei.catalog.outbox.action")?,
            resource: node_text(node, "hodei.catalog.outbox.resource")?,
            digest: node_option_text(node, "hodei.catalog.outbox.digest"),
            result: node_text(node, "hodei.catalog.outbox.result")?,
            occurred_at: node_time(node, "hodei.catalog.outbox.occurred_at")?,
            state: node_text(node, "hodei.catalog.outbox.state")?,
            attempts: node
                .get("hodei.catalog.outbox.attempts")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            lease_owner: node_option_text(node, "hodei.catalog.outbox.lease_owner"),
            lease_until: node_option_time(node, "hodei.catalog.outbox.lease_until"),
            last_error: node_option_text(node, "hodei.catalog.outbox.last_error"),
            delivered_at: node_option_time(node, "hodei.catalog.outbox.delivered_at"),
        })
    }

    /// El evento de la fila, reconstruido.
    fn the_event(&self) -> AuditEvent {
        AuditEvent::new(
            self.event_id.clone(),
            AuditContext::new(
                self.actor.clone(),
                self.tenant.clone(),
                self.correlation_id.clone(),
            ),
            self.occurred_at,
            self.action.clone(),
            self.resource.clone(),
            self.digest.clone(),
            the_result(&self.result),
        )
    }

    /// La entrada de la fila, con su estado.
    fn the_entry(&self) -> AuditEntry {
        AuditEntry {
            event: self.the_event(),
            state: the_state(&self.state),
            attempts: u32::try_from(self.attempts).unwrap_or(u32::MAX),
            lease_owner: self.lease_owner.clone(),
            lease_until: self.lease_until,
            last_error: self.last_error.clone(),
            delivered_at: self.delivered_at,
        }
    }
}

/// La falta del almacenamiento, dicha como error de auditoría.
fn the_audit_error(error: impl std::fmt::Display) -> AuditError {
    AuditError::StoreUnavailable {
        detail: error.to_string(),
    }
}

/// El outbox de auditoría sobre el grafo. El lado de la **escritura** lo
/// tiene el catálogo (la fila del evento se escribe dentro de la
/// transacción que publica); lo que hay aquí es el lado de la **entrega**:
/// reclamar un evento, marcarlo entregado y devolverlo al mercado cuando
/// la entrega no ocurrió, cada uno con su compare-and-set de una pieza.
#[derive(Clone)]
pub struct DgraphAuditOutbox {
    client: Arc<GraphClient>,
}

impl std::fmt::Debug for DgraphAuditOutbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DgraphAuditOutbox").finish_non_exhaustive()
    }
}

impl DgraphAuditOutbox {
    /// El outbox sobre un cliente ya conectado y autenticado.
    pub fn new(client: Arc<GraphClient>) -> Self {
        Self { client }
    }

    /// La fila de un evento, si está.
    async fn the_row(&self, event_id: &str) -> Result<Option<OutboxRow>, AuditError> {
        let query = format!(
            "{{ q(func: eq(hodei.catalog.outbox.key, {})) {{ {OUTBOX_FIELDS} }} }}",
            dql_string(event_id),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(the_audit_error)?;
        match single_row(response.json.get("q"), "outbox", event_id.to_string()) {
            Ok(None) => Ok(None),
            Ok(Some(node)) => OutboxRow::from_node(&node)
                .map(Some)
                .map_err(the_audit_error),
            Err(error) => Err(AuditError::StoreUnavailable {
                detail: error.to_string(),
            }),
        }
    }

    /// El upsert que dice un evento una sola vez: el índice `@upsert` de
    /// la clave decide entre dos escritores, y la relectura decide si el
    /// segundo decía lo mismo.
    fn put_upsert(&self, event: &AuditEvent) -> hodei_graph_client::GraphUpsertTransaction {
        hodei_graph_client::GraphUpsertTransaction {
            query: format!(
                "{{ ev as var(func: eq(hodei.catalog.outbox.key, {}))\n \
                 evq(func: eq(hodei.catalog.outbox.key, {})) {{ hodei.catalog.outbox.key }} }}",
                dql_string(&event.event_id),
                dql_string(&event.event_id),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(ev), 0)",
                outbox_event_nquads(event),
            )],
            commit_now: true,
        }
    }

    /// El upsert que reclama un evento pendiente (o cuyo lease expiró).
    fn claim_upsert(
        &self,
        event_id: &str,
        worker: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> hodei_graph_client::GraphUpsertTransaction {
        let until = lease_until.to_rfc3339_opts(SecondsFormat::Millis, true);
        let at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let claim = |binding: &str| {
            format!(
                "uid({binding}) <hodei.catalog.outbox.state> \"claimed\" .\n\
                 uid({binding}) <hodei.catalog.outbox.lease_owner> {} .\n\
                 uid({binding}) <hodei.catalog.outbox.lease_until> \"{until}\" .\n",
                dql_string(worker),
            )
        };
        hodei_graph_client::GraphUpsertTransaction {
            query: format!(
                "{{ ep as var(func: eq(hodei.catalog.outbox.key, {})) @filter(eq(hodei.catalog.outbox.state, \"pending\"))\n \
                 ex as var(func: eq(hodei.catalog.outbox.key, {})) @filter(eq(hodei.catalog.outbox.state, \"claimed\") AND le(hodei.catalog.outbox.lease_until, \"{at}\")) }}",
                dql_string(event_id),
                dql_string(event_id),
            ),
            blocks: vec![
                GraphMutationBlock::set_if("eq(len(ep), 1)", claim("ep")),
                GraphMutationBlock::set_if("eq(len(ep), 0) AND eq(len(ex), 1)", claim("ex")),
            ],
            commit_now: true,
        }
    }

    /// El upsert que marca la entrega, con el worker en la condición: un
    /// lease que expiró ya no es un permiso para marcar nada.
    fn mark_delivered_upsert(
        &self,
        event_id: &str,
        worker: &str,
        at: DateTime<Utc>,
    ) -> hodei_graph_client::GraphUpsertTransaction {
        let stamp = at.to_rfc3339_opts(SecondsFormat::Millis, true);
        hodei_graph_client::GraphUpsertTransaction {
            query: format!(
                "{{ md as var(func: eq(hodei.catalog.outbox.key, {})) @filter(eq(hodei.catalog.outbox.state, \"claimed\") AND eq(hodei.catalog.outbox.lease_owner, {})) }}",
                dql_string(event_id),
                dql_string(worker),
            ),
            blocks: vec![GraphMutationBlock {
                cond: Some("eq(len(md), 1)".to_owned()),
                set_nquads: format!(
                    "uid(md) <hodei.catalog.outbox.state> \"delivered\" .\n\
                     uid(md) <hodei.catalog.outbox.delivered_at> \"{stamp}\" .\n",
                ),
                delete_nquads: "uid(md) <hodei.catalog.outbox.lease_owner> * .\n\
                     uid(md) <hodei.catalog.outbox.lease_until> * .\n"
                    .to_owned(),
            }],
            commit_now: true,
        }
    }

    /// El upsert que devuelve un evento al mercado, con el worker y los
    /// intentos ya contados en la condición: un compare-and-set de una
    /// pieza que no puede desmentir la entrega de otro.
    fn mark_undelivered_upsert(
        &self,
        event_id: &str,
        worker: &str,
        detail: &str,
        attempts: u64,
    ) -> hodei_graph_client::GraphUpsertTransaction {
        let next = attempts + 1;
        hodei_graph_client::GraphUpsertTransaction {
            query: format!(
                "{{ mu as var(func: eq(hodei.catalog.outbox.key, {})) @filter(eq(hodei.catalog.outbox.state, \"claimed\") AND eq(hodei.catalog.outbox.lease_owner, {}) AND eq(hodei.catalog.outbox.attempts, {attempts})) }}",
                dql_string(event_id),
                dql_string(worker),
            ),
            blocks: vec![GraphMutationBlock {
                cond: Some("eq(len(mu), 1)".to_owned()),
                set_nquads: format!(
                    "uid(mu) <hodei.catalog.outbox.state> \"pending\" .\n\
                     uid(mu) <hodei.catalog.outbox.attempts> \"{next}\" .\n\
                     uid(mu) <hodei.catalog.outbox.last_error> {} .\n",
                    dql_string(&hodei_audit::sanitize_text(detail)),
                ),
                delete_nquads: "uid(mu) <hodei.catalog.outbox.lease_owner> * .\n\
                     uid(mu) <hodei.catalog.outbox.lease_until> * .\n"
                    .to_owned(),
            }],
            commit_now: true,
        }
    }
}

#[async_trait]
impl AuditOutbox for DgraphAuditOutbox {
    async fn put_pending(&self, event: &AuditEvent) -> Result<(), AuditError> {
        // Decir un evento dos veces es decirlo una. Antes de culpar al
        // almacenamiento se mira lo que hay.
        if let Some(existing) = self.the_row(&event.event_id).await? {
            return if existing.the_event().same_fact(event) {
                Ok(())
            } else {
                Err(AuditError::StoreUnavailable {
                    detail: format!(
                        "the event {} is already said with another content",
                        event.event_id
                    ),
                })
            };
        }
        for _attempt in 0..5 {
            match self.client.upsert(self.put_upsert(event)).await {
                Ok(response) => {
                    // El bloque solo se aplica si la fila estaba libre
                    // DENTRO de la transacción: si no lo estuvo, la fila
                    // apareció entre la lectura y la escritura, y lo que
                    // dice la fila decide.
                    let applied = response
                        .json
                        .get("evq")
                        .and_then(serde_json::Value::as_array)
                        .is_some_and(|nodes| nodes.is_empty());
                    if applied {
                        return Ok(());
                    }
                    if let Some(existing) = self.the_row(&event.event_id).await? {
                        return if existing.the_event().same_fact(event) {
                            Ok(())
                        } else {
                            Err(AuditError::StoreUnavailable {
                                detail: format!(
                                    "the event {} is already said with another content",
                                    event.event_id
                                ),
                            })
                        };
                    }
                }
                Err(GraphClientError::UpsertConflict { detail }) => {
                    // Dos escritores dijeron el mismo identificador a la
                    // vez: el índice decidió; la relectura dice si el
                    // segundo decía lo mismo.
                    tracing::debug!(event_id = %event.event_id, %detail, "the outbox write conflicted");
                }
                Err(other) => return Err(the_audit_error(other)),
            }
        }
        Err(AuditError::StoreUnavailable {
            detail: "the event could not be said after the attempts".to_owned(),
        })
    }

    async fn claim(
        &self,
        event_id: &str,
        worker: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<AuditEntry>, AuditError> {
        self.client
            .upsert(self.claim_upsert(event_id, worker, lease_until, now))
            .await
            .map_err(the_audit_error)?;
        // El estado después del commit responde: dos workers piden lo
        // mismo y el que pierde ve que la fila ya no es suya.
        let row = self.the_row(event_id).await?;
        Ok(row
            .filter(|row| {
                row.state == EntryState::Claimed.as_str()
                    && row.lease_owner.as_deref() == Some(worker)
            })
            .map(|row| row.the_entry()))
    }

    async fn mark_delivered(
        &self,
        event_id: &str,
        worker: &str,
        at: DateTime<Utc>,
    ) -> Result<(), AuditError> {
        // Lo que ya es verdad no se cambia.
        if let Some(row) = self.the_row(event_id).await? {
            if row.state == EntryState::Delivered.as_str() {
                return Ok(());
            }
            if row.state != EntryState::Claimed.as_str() {
                return Err(AuditError::StoreUnavailable {
                    detail: format!("the event {event_id} is not claimed, so nobody delivered it"),
                });
            }
        }
        self.client
            .upsert(self.mark_delivered_upsert(event_id, worker, at))
            .await
            .map_err(the_audit_error)?;
        let row = self.the_row(event_id).await?;
        match row {
            Some(row) if row.state == EntryState::Delivered.as_str() => Ok(()),
            _ => Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is not claimed by {worker}, so nobody delivered it"
                ),
            }),
        }
    }

    async fn mark_undelivered(
        &self,
        event_id: &str,
        worker: &str,
        detail: &str,
    ) -> Result<(), AuditError> {
        // Una entrega que ya ocurrió no se puede desmentir.
        let Some(row) = self.the_row(event_id).await? else {
            return Err(AuditError::StoreUnavailable {
                detail: format!("the event {event_id} is not there, so nobody can say it failed"),
            });
        };
        if row.state == EntryState::Delivered.as_str() {
            return Ok(());
        }
        if row.state != EntryState::Claimed.as_str() {
            return Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is not claimed by {worker}, so nobody can say it failed"
                ),
            });
        }
        self.client
            .upsert(self.mark_undelivered_upsert(event_id, worker, detail, row.attempts))
            .await
            .map_err(the_audit_error)?;
        let row = self.the_row(event_id).await?;
        match row {
            Some(row) if row.state == EntryState::Pending.as_str() => Ok(()),
            _ => Err(AuditError::StoreUnavailable {
                detail: format!(
                    "the event {event_id} is not claimed by {worker}, so nobody can say it failed"
                ),
            }),
        }
    }

    async fn pending(&self, now: DateTime<Utc>) -> Result<Vec<AuditEntry>, AuditError> {
        let at = now.to_rfc3339_opts(SecondsFormat::Millis, true);
        let query = format!(
            "{{ pend(func: eq(hodei.catalog.outbox.state, \"pending\")) {{ {OUTBOX_FIELDS} }}\n \
             expired(func: eq(hodei.catalog.outbox.state, \"claimed\")) @filter(le(hodei.catalog.outbox.lease_until, \"{at}\")) {{ {OUTBOX_FIELDS} }} }}"
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(the_audit_error)?;
        let mut entries = Vec::new();
        for root in ["pend", "expired"] {
            let nodes = response
                .json
                .get(root)
                .and_then(serde_json::Value::as_array)
                .cloned()
                .unwrap_or_default();
            for node in &nodes {
                let row = OutboxRow::from_node(node).map_err(the_audit_error)?;
                entries.push(row.the_entry());
            }
        }
        entries.sort_by(|left, right| left.event.event_id.cmp(&right.event.event_id));
        Ok(entries)
    }

    async fn entry(&self, event_id: &str) -> Result<Option<AuditEntry>, AuditError> {
        Ok(self.the_row(event_id).await?.map(|row| row.the_entry()))
    }
}
