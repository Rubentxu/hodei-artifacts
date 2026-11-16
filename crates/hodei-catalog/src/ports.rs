//! Los puertos del catálogo.
//!
//! El catálogo habla con el mundo por cuatro puertos: leer, escribir,
//! decidir quien puede hacer cada cosa, y preguntar por la integridad de
//! los bytes que el catálogo referencia. Ninguno de ellos nombra una base
//! de datos: ese conocimiento vive en el crate de adaptadores.

use async_trait::async_trait;
use hodei_evidence::ContentDigest;
use thiserror::Error;

use crate::error::CatalogError;
use crate::model::{
    AssetRecord, BlobLocation, BlobReference, MutableTag, PackageCoordinates, PackageVersion,
    Publication, PublishOutcome, ReferrerRecord, RepositoryConfig, RepositoryDescriptor,
};
use crate::search::{SearchPage, SearchQuery};

/// Lo que un principal quiere hacer con el catálogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogAction {
    /// Leer un asset, una versión o una etiqueta.
    Read,
    /// Publicar contenido.
    Publish,
    /// Mover una etiqueta.
    MoveTag,
    /// Mover un asset a otra versión.
    MoveAsset,
    /// Crear o reconfigurar un repositorio.
    Configure,
}

impl CatalogAction {
    /// El verbo, para los mensajes de error.
    pub fn as_str(&self) -> &'static str {
        match self {
            CatalogAction::Read => "read",
            CatalogAction::Publish => "publish",
            CatalogAction::MoveTag => "move the tag",
            CatalogAction::MoveAsset => "move the asset",
            CatalogAction::Configure => "configure",
        }
    }
}

/// La clase de recurso sobre la que se pide permiso.
///
/// El decisor no solo necesita saber si el principal puede leer: necesita
/// saber **que** lee. Permitir la lectura de un asset y la de una etiqueta
/// son dos decisiones distintas, y un decisor que solo ve "read" no puede
/// distinguirlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogResource {
    /// El repositorio.
    Repository,
    /// Una versión publicada.
    Version,
    /// Un asset, identified por su ruta.
    Asset,
    /// Una etiqueta mutable.
    Tag,
}

impl CatalogResource {
    /// El nombre de la clase, que es también el `kind` del error `Denied`.
    pub fn as_str(&self) -> &'static str {
        match self {
            CatalogResource::Repository => "repository",
            CatalogResource::Version => "version",
            CatalogResource::Asset => "asset",
            CatalogResource::Tag => "tag",
        }
    }
}

/// La petición que se decide.
///
/// Lleva el principal y las coordenadas de lo que se pide, y nada de lo
/// que existe. La decision se toma sobre lo que se pide, no sobre lo que
/// hay: si la decision dependiera de la existencia, responder `Denied` a
/// un objeto de otra cuenta ya sería filtrar su existencia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRequest {
    /// La cuenta del principal verificado.
    pub principal_account: String,
    /// Lo que quiere hacer.
    pub action: CatalogAction,
    /// La clase de recurso sobre la que se decide.
    pub resource: CatalogResource,
    /// El repositorio.
    pub repository: String,
    /// La clave del recurso dentro del repositorio: la clave del
    /// repositorio, la ruta del asset, las coordenadas del paquete o el
    /// nombre de la etiqueta, según la clase del recurso.
    pub subject: String,
}

impl AccessRequest {
    /// La petición sobre un repositorio.
    pub fn repository(principal_account: &str, action: CatalogAction, repo_key: &str) -> Self {
        Self {
            principal_account: principal_account.to_string(),
            action,
            resource: CatalogResource::Repository,
            repository: repo_key.to_string(),
            subject: repo_key.to_string(),
        }
    }

    /// La petición sobre un asset, por su ruta.
    pub fn asset(
        principal_account: &str,
        action: CatalogAction,
        repository: &str,
        asset_path: &str,
    ) -> Self {
        Self {
            principal_account: principal_account.to_string(),
            action,
            resource: CatalogResource::Asset,
            repository: repository.to_string(),
            subject: asset_path.to_string(),
        }
    }

    /// La petición sobre una versión, por sus coordenadas.
    pub fn version(
        principal_account: &str,
        action: CatalogAction,
        repository: &str,
        coordinates: &PackageCoordinates,
    ) -> Self {
        Self {
            principal_account: principal_account.to_string(),
            action,
            resource: CatalogResource::Version,
            repository: repository.to_string(),
            subject: format!(
                "{}@{}",
                coordinates.package_name, coordinates.package_version
            ),
        }
    }

    /// La petición sobre una etiqueta, por su nombre.
    pub fn tag(
        principal_account: &str,
        action: CatalogAction,
        repository: &str,
        tag_name: &str,
    ) -> Self {
        Self {
            principal_account: principal_account.to_string(),
            action,
            resource: CatalogResource::Tag,
            repository: repository.to_string(),
            subject: tag_name.to_string(),
        }
    }

    /// La petición de publicación.
    pub fn publish(principal_account: &str, repository: &str, asset_path: &str) -> Self {
        Self::asset(
            principal_account,
            CatalogAction::Publish,
            repository,
            asset_path,
        )
    }
}

/// Lo que se decide sobre una petición.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecision {
    /// Se permite.
    Allow,
    /// Se prohibe, con el motivo.
    Deny(String),
}

/// Quien decide si una petición se atiende.
///
/// El catálogo no sabe de Cedar, ni de roles, ni de tokens: solo sabe
/// preguntar. Quien decide se inyecta, y el valor por defecto es
/// denegar: un catálogo al que no se le ha puesto un decisor no publica
/// nada.
#[async_trait]
pub trait CatalogAuthorizer: Send + Sync {
    /// Decide sobre la petición.
    async fn authorize(&self, request: &AccessRequest) -> Result<AccessDecision, CatalogError>;
}

/// El decisor que no permite nada.
///
/// Es el valor por defecto de un catálogo sin decisor, y por eso es
/// también el que se usa para comprobar que el camino sin permisos está
/// cerrado de verdad.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyAllAuthorizer;

#[async_trait]
impl CatalogAuthorizer for DenyAllAuthorizer {
    async fn authorize(&self, request: &AccessRequest) -> Result<AccessDecision, CatalogError> {
        Ok(AccessDecision::Deny(format!(
            "the catalog has no authorizer configured, so {} is refused",
            request.action.as_str()
        )))
    }
}

/// El decisor que permite todo.
///
/// Solo para el servicio interno, que ya autorizó la petición antes de
/// llegar aquí. No es un atajo de las pruebas: es la frontera del
/// catálogo, que no vuelve a decidir lo que la frontera ya decidió.
#[derive(Debug, Clone, Copy, Default)]
pub struct AllowAllAuthorizer;

#[async_trait]
impl CatalogAuthorizer for AllowAllAuthorizer {
    async fn authorize(&self, _request: &AccessRequest) -> Result<AccessDecision, CatalogError> {
        Ok(AccessDecision::Allow)
    }
}

/// Lo que puede fallar al preguntar por un blob.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BlobProbeError {
    /// El blob no está en el almacén.
    #[error("the blob is not in the store")]
    Missing,
    /// El almacén no pudo responder.
    #[error("the blob store is unavailable: {detail}")]
    Unavailable {
        /// Lo que dijo el almacén.
        detail: String,
    },
}

/// La longitud real de un blob en el CAS.
///
/// El catálogo guarda la longitud que declaró el cliente. Esta es la
/// longitud que tiene el contenido de verdad, y compararlas es lo que
/// separa un registro sano de uno roto.
///
/// La pregunta lleva **la ubicación** del blob y no solo su digest, porque
/// la clave con la que el CAS guarda los bytes depende del ámbito: el
/// mismo digest vive en una ranura global o en una particionada por
/// cuenta. Preguntar por el digest a secas solo podría encontrar el
/// contenido global, y daría por roto un blob particionado que está
/// sano.
///
/// Lo que la sonda NO hace es recalcular el digest: comprueba la
/// longitud, que es barato, y deja la verificación profunda al scrub.
#[async_trait]
pub trait BlobIntegrityProbe: Send + Sync {
    /// La longitud del blob, en bytes.
    async fn length_of(&self, location: &BlobLocation) -> Result<u64, BlobProbeError>;
}

/// Los assets como maximo que devuelve `assets_under`.
pub const MAX_ASSETS_UNDER: usize = 20_000;

/// La lectura del catálogo.
///
/// Todas las lecturas llevan la cuenta del principal verificado. La cuenta
/// no es un filtro que se pueda olvidar: es parte de la consulta, y por
/// eso dos cuentas con el mismo camino no se ven.
#[async_trait]
pub trait CatalogReader: Send + Sync {
    /// El repositorio de la cuenta.
    async fn repository(
        &self,
        principal_account: &str,
        repo_key: &str,
    ) -> Result<RepositoryDescriptor, CatalogError>;

    /// Los assets cuya ruta empieza por el prefijo, ordenados por ruta.
    ///
    /// Es la lectura que necesitan los formatos que **derivan** vistas de lo
    /// publicado (por ejemplo, los metadatos de Maven). No comprueba que cada
    /// asset apunte a una version que exista: eso lo hace `asset`, que es el
    /// que sirve contenido. Si hay mas de `MAX_ASSETS_UNDER`, falla en vez de
    /// devolver una lista que parece completa y no lo es.
    async fn assets_under(
        &self,
        principal_account: &str,
        repository: &str,
        prefix: &str,
    ) -> Result<Vec<AssetRecord>, CatalogError>;

    /// El asset en una ruta del repositorio.
    async fn asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<AssetRecord, CatalogError>;

    /// La versión publicada.
    async fn version(
        &self,
        principal_account: &str,
        repository: &str,
        coordinates: &PackageCoordinates,
    ) -> Result<PackageVersion, CatalogError>;

    /// La etiqueta y la versión a la que apunta.
    async fn tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
    ) -> Result<MutableTag, CatalogError>;

    /// La referencia viva al blob.
    async fn reference(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<BlobReference, CatalogError>;

    /// Las etiquetas del repositorio.
    ///
    /// La lista completa, sin pagina: el que pagina es el que sabe lo
    /// que el cliente ha visto ya, y el corte lo hace la superficie que
    /// habla el protocolo.
    async fn tags(
        &self,
        principal_account: &str,
        repository: &str,
    ) -> Result<Vec<MutableTag>, CatalogError>;

    /// Los referrers de un manifiesto: los manifiestos de la cuenta y el
    /// repositorio cuyo `subject` es el digest dado.
    ///
    /// Una lista vacía es una respuesta valida: un `subject` que no tiene
    /// referrers no es un error, y el protocolo lo dice asi.
    async fn referrers(
        &self,
        principal_account: &str,
        repository: &str,
        subject: &ContentDigest,
    ) -> Result<Vec<ReferrerRecord>, CatalogError>;
}

/// The catalog search.
///
/// It is a port apart from the read one, and not an extension of it: "what
/// is there?" and "what is there at this path?" are different questions,
/// with different limits and different pages.
///
/// # The permission comes first
///
/// The search receives the repositories the principal may read, and only
/// looks in them. The account is not a filter that can be forgotten: it
/// is part of the query, and a total that counted what cannot be read
/// would say there is something where there is nothing to see.
#[async_trait]
pub trait CatalogSearch: Send + Sync {
    /// The repositories of the account.
    ///
    /// The ones the principal may read are decided outside: here only the
    /// ones of the account are listed, which is what the permission can
    /// narrow.
    async fn repositories(
        &self,
        principal_account: &str,
    ) -> Result<Vec<RepositoryDescriptor>, CatalogError>;

    /// The page of results of a search.
    ///
    /// Los repositorios son los que el principal puede leer. Un principal
    /// que no puede leer ninguno no ve nada, y su total es cero.
    async fn search(
        &self,
        principal_account: &str,
        repositories: &[RepositoryDescriptor],
        query: &SearchQuery,
    ) -> Result<SearchPage, CatalogError>;
}

/// La escritura del catálogo.
#[async_trait]
pub trait CatalogWriter: Send + Sync {
    /// Crea un repositorio.
    async fn create_repository(
        &self,
        principal_account: &str,
        descriptor: RepositoryDescriptor,
    ) -> Result<RepositoryDescriptor, CatalogError>;

    /// Cambia la configuración del repositorio y sube su versión.
    async fn update_config(
        &self,
        principal_account: &str,
        repo_key: &str,
        config: RepositoryConfig,
    ) -> Result<RepositoryDescriptor, CatalogError>;

    /// Publica contenido: versión, asset, referencia y etiquetas, en una
    /// sola operación.
    async fn publish(
        &self,
        principal_account: &str,
        publication: Publication,
    ) -> Result<PublishOutcome, CatalogError>;

    /// Mueve una etiqueta a otra versión.
    ///
    /// Mover la etiqueta no toca el blob de ninguna de las dos versiones.
    async fn move_tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
        coordinates: PackageCoordinates,
    ) -> Result<MutableTag, CatalogError>;

    /// Mueve un asset a otra versión.
    ///
    /// La ruta de un repositorio es un índice: se puede mover, pero moverlo
    /// es una operación explícita y no un efecto colateral de publicar. Por
    /// eso publicar en una ruta ocupada es un conflicto, y esta es la
    /// operación que resuelve ese conflicto cuando el cliente lo pide.
    ///
    /// Mover el asset mueve también la referencia al blob: la referencia
    /// tiene que apuntar al contenido al que apunta el asset, o la
    /// descarga serviría un blob y la recolección guardaría otro.
    async fn move_asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
        coordinates: PackageCoordinates,
    ) -> Result<AssetRecord, CatalogError>;
}
