//! Los puertos del catalogo.
//!
//! El catalogo habla con el mundo por cuatro puertos: leer, escribir,
//! decidir quien puede hacer cada cosa, y preguntar por la integridad de
//! los bytes que el catalogo referencia. Ninguno de ellos nombra una base
//! de datos: ese conocimiento vive en el crate de adaptadores.

use async_trait::async_trait;
use thiserror::Error;

use crate::error::CatalogError;
use crate::model::{
    AssetRecord, BlobLocation, BlobReference, MutableTag, PackageCoordinates, PackageVersion,
    Publication, PublishOutcome, RepositoryConfig, RepositoryDescriptor,
};

/// Lo que un principal quiere hacer con el catalogo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogAction {
    /// Leer un asset, una version o una etiqueta.
    Read,
    /// Publicar contenido.
    Publish,
    /// Mover una etiqueta.
    MoveTag,
    /// Mover un asset a otra version.
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
    /// Una version publicada.
    Version,
    /// Un asset, identified por su ruta.
    Asset,
    /// Una etiqueta mutable.
    Tag,
}

impl CatalogResource {
    /// El nombre de la clase, que es tambien el `kind` del error `Denied`.
    pub fn as_str(&self) -> &'static str {
        match self {
            CatalogResource::Repository => "repository",
            CatalogResource::Version => "version",
            CatalogResource::Asset => "asset",
            CatalogResource::Tag => "tag",
        }
    }
}

/// La peticion que se decide.
///
/// Lleva el principal y las coordenadas de lo que se pide, y nada de lo
/// que existe. La decision se toma sobre lo que se pide, no sobre lo que
/// hay: si la decision dependiera de la existencia, responder `Denied` a
/// un objeto de otra cuenta ya seria filtrar su existencia.
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
    /// nombre de la etiqueta, segun la clase del recurso.
    pub subject: String,
}

impl AccessRequest {
    /// La peticion sobre un repositorio.
    pub fn repository(principal_account: &str, action: CatalogAction, repo_key: &str) -> Self {
        Self {
            principal_account: principal_account.to_string(),
            action,
            resource: CatalogResource::Repository,
            repository: repo_key.to_string(),
            subject: repo_key.to_string(),
        }
    }

    /// La peticion sobre un asset, por su ruta.
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

    /// La peticion sobre una version, por sus coordenadas.
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

    /// La peticion sobre una etiqueta, por su nombre.
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

    /// La peticion de publicacion.
    pub fn publish(principal_account: &str, repository: &str, asset_path: &str) -> Self {
        Self::asset(
            principal_account,
            CatalogAction::Publish,
            repository,
            asset_path,
        )
    }
}

/// Lo que se decide sobre una peticion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessDecision {
    /// Se permite.
    Allow,
    /// Se prohibe, con el motivo.
    Deny(String),
}

/// Quien decide si una peticion se atiende.
///
/// El catalogo no sabe de Cedar, ni de roles, ni de tokens: solo sabe
/// preguntar. Quien decide se inyecta, y el valor por defecto es
/// denegar: un catalogo al que no se le ha puesto un decisor no publica
/// nada.
#[async_trait]
pub trait CatalogAuthorizer: Send + Sync {
    /// Decide sobre la peticion.
    async fn authorize(&self, request: &AccessRequest) -> Result<AccessDecision, CatalogError>;
}

/// El decisor que no permite nada.
///
/// Es el valor por defecto de un catalogo sin decisor, y por eso es
/// tambien el que se usa para comprobar que el camino sin permisos esta
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
/// Solo para el servicio interno, que ya autorizo la peticion antes de
/// llegar aqui. No es un atajo de las pruebas: es la frontera del
/// catalogo, que no vuelve a decidir lo que la frontera ya decidio.
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
    /// El blob no esta en el almacen.
    #[error("the blob is not in the store")]
    Missing,
    /// El almacen no pudo responder.
    #[error("the blob store is unavailable: {detail}")]
    Unavailable {
        /// Lo que dijo el almacen.
        detail: String,
    },
}

/// La longitud real de un blob en el CAS.
///
/// El catalogo guarda la longitud que declaro el cliente. Esta es la
/// longitud que tiene el contenido de verdad, y compararlas es lo que
/// separa un registro sano de uno roto.
///
/// La pregunta lleva **la ubicacion** del blob y no solo su digest, porque
/// la clave con la que el CAS guarda los bytes depende del ambito: el
/// mismo digest vive en una ranura global o en una particionada por
/// cuenta. Preguntar por el digest a secas solo podria encontrar el
/// contenido global, y daria por roto un blob particionado que esta
/// sano.
///
/// Lo que la sonda NO hace es recalcular el digest: comprueba la
/// longitud, que es barato, y deja la verificacion profunda al scrub.
#[async_trait]
pub trait BlobIntegrityProbe: Send + Sync {
    /// La longitud del blob, en bytes.
    async fn length_of(&self, location: &BlobLocation) -> Result<u64, BlobProbeError>;
}

/// La lectura del catalogo.
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

    /// El asset en una ruta del repositorio.
    async fn asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
    ) -> Result<AssetRecord, CatalogError>;

    /// La version publicada.
    async fn version(
        &self,
        principal_account: &str,
        repository: &str,
        coordinates: &PackageCoordinates,
    ) -> Result<PackageVersion, CatalogError>;

    /// La etiqueta y la version a la que apunta.
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
}

/// La escritura del catalogo.
#[async_trait]
pub trait CatalogWriter: Send + Sync {
    /// Crea un repositorio.
    async fn create_repository(
        &self,
        principal_account: &str,
        descriptor: RepositoryDescriptor,
    ) -> Result<RepositoryDescriptor, CatalogError>;

    /// Cambia la configuracion del repositorio y sube su version.
    async fn update_config(
        &self,
        principal_account: &str,
        repo_key: &str,
        config: RepositoryConfig,
    ) -> Result<RepositoryDescriptor, CatalogError>;

    /// Publica contenido: version, asset, referencia y etiquetas, en una
    /// sola operacion.
    async fn publish(
        &self,
        principal_account: &str,
        publication: Publication,
    ) -> Result<PublishOutcome, CatalogError>;

    /// Mueve una etiqueta a otra version.
    ///
    /// Mover la etiqueta no toca el blob de ninguna de las dos versiones.
    async fn move_tag(
        &self,
        principal_account: &str,
        repository: &str,
        tag_name: &str,
        coordinates: PackageCoordinates,
    ) -> Result<MutableTag, CatalogError>;

    /// Mueve un asset a otra version.
    ///
    /// La ruta de un repositorio es un indice: se puede mover, pero moverlo
    /// es una operacion explicita y no un efecto colateral de publicar. Por
    /// eso publicar en una ruta ocupada es un conflicto, y esta es la
    /// operacion que resuelve ese conflicto cuando el cliente lo pide.
    ///
    /// Mover el asset mueve tambien la referencia al blob: la referencia
    /// tiene que apuntar al contenido al que apunta el asset, o la
    /// descarga serviria un blob y la recoleccion guardaria otro.
    async fn move_asset(
        &self,
        principal_account: &str,
        repository: &str,
        asset_path: &str,
        coordinates: PackageCoordinates,
    ) -> Result<AssetRecord, CatalogError>;
}
