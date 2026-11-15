//! El modelo del catalogo.
//!
//! Aqui se separan las cuatro cosas que un repositorio de artefactos
//! confunde con facilidad, y que este catalogo mantiene separadas:
//!
//! 1. **La identidad del repositorio**: cuenta, clave, formato, modo y
//!    configuracion versionada.
//! 2. **La identidad del contenido**: el digest, que no depende de donde
//!    vive ni de quien lo publico.
//! 3. **Las coordenadas del paquete**: el nombre y la version, que son
//!    inmutables una vez publicadas.
//! 4. **La ruta del asset**: la posicion dentro del repositorio, que es lo
//!    que el cliente pide y lo que puede apuntar a otra version sin tocar
//!    los bytes.
//!
//! La separacion no es estetica. Es lo que permite que una etiqueta
//! mutable se mueva sin alterar un blob, y que dos cuentas usen el mismo
//! camino sin pisarse.

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// El formato del contenido que guarda un repositorio.
///
/// El catalogo los conoce todos porque tiene que poder describir un
/// repositorio antes de que exista el adaptador que lo sirve. Un formato
/// que aun no se sirve no es un error: es un repositorio que se declara y
/// que espera su turno.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryFormat {
    /// Contenido opaco, served tal cual. Es el formato de laRaw 1.0.
    Raw,
    /// Imagenes OCI, con manifests y tags.
    Oci,
    /// Maven2, con `maven-metadata.xml` y layouts de grupo.
    Maven2,
    /// npm, con packuments, scopes y dist-tags.
    Npm,
    /// PyPI, con la Simple API.
    PyPi,
    /// Cargo, con el indice sparse.
    Cargo,
    /// NuGet v3, con service index y flat container.
    NuGet,
    /// Go Modules, con el protocolo GOPROXY.
    Go,
    /// Helm, con charts y repositorio de indices.
    Helm,
    /// APT, con indices y firmas.
    Apt,
    /// RPM, con creacion de repositorios.
    Rpm,
}

impl RepositoryFormat {
    /// El nombre canonico del formato, el que va al cable y a la base.
    pub fn as_str(&self) -> &'static str {
        match self {
            RepositoryFormat::Raw => "raw",
            RepositoryFormat::Oci => "oci",
            RepositoryFormat::Maven2 => "maven2",
            RepositoryFormat::Npm => "npm",
            RepositoryFormat::PyPi => "pypi",
            RepositoryFormat::Cargo => "cargo",
            RepositoryFormat::NuGet => "nuget",
            RepositoryFormat::Go => "go",
            RepositoryFormat::Helm => "helm",
            RepositoryFormat::Apt => "apt",
            RepositoryFormat::Rpm => "rpm",
        }
    }
}

/// Como se comporta un repositorio respecto del exterior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryMode {
    /// El repositorio guarda lo que se publica en el.
    Hosted,
    /// El repositorio resuelve contra un upstream y cachea lo que baja.
    Proxy,
    /// El repositorio agrupa otros repositorios y resuelve en ellos.
    Group,
}

impl std::fmt::Display for RepositoryMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl RepositoryMode {
    /// El nombre canonico del modo, el que va al cable y a la base.
    pub fn as_str(&self) -> &'static str {
        match self {
            RepositoryMode::Hosted => "hosted",
            RepositoryMode::Proxy => "proxy",
            RepositoryMode::Group => "group",
        }
    }

    /// Si esta release sabe comportarse asi.
    ///
    /// El catalogo sabe DESCRIBIR los tres modos, porque un repositorio se
    /// declara antes de existir. Lo que no puede es SERVIR un proxy sin
    /// resolver contra el upstream, ni un group sin resolver en varios
    /// repositorios: prometerlo aqui y no cumplirlos seria una promesa
    /// falsa, y una promesa falsa en un registro de artefactos se paga
    /// cuando alguien publica en un repositorio que no existe.
    pub fn is_served_by_this_release(&self) -> bool {
        matches!(self, RepositoryMode::Hosted)
    }
}

/// Donde vive el blob de un digest.
///
/// El digest identifica el contenido, pero no dice donde se guardan sus
/// bytes. Esa es una decision de la cuenta, y por eso es politica y no
/// detalle de implementacion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestScope {
    /// Un solo blob por digest, compartido por todas las cuentas.
    ///
    /// Dos cuentas que publican el mismo contenido comparten los bytes.
    /// Es lo que hace un registro eficiente, y tambien lo que hace que el
    /// borrado de un blob tenga que tener en cuenta a todos.
    Global,
    /// Un blob por digest y cuenta: el mismo contenido en dos cuentas son
    /// dos juegos de bytes.
    ///
    /// Es lo que necesita una cuenta que no quiere que su contenido se
    /// pueda deduplicar contra el de otra cuenta.
    AccountPartitioned,
}

impl DigestScope {
    /// La clave con la que el CAS guarda el blob.
    ///
    /// El CAS guarda los digests SHA-256 por su valor hexadecimal, asi que
    /// la clave global es ese valor. La forma particionada antepone la
    /// cuenta, y por eso las dos politicas no pueden colar el mismo blob
    /// en la misma ranura.
    pub fn blob_key(&self, account: &str, digest: &ContentDigest) -> String {
        match self {
            DigestScope::Global => digest.value.clone(),
            DigestScope::AccountPartitioned => format!("{account}/{}", digest.value),
        }
    }
}

/// La configuracion de un repositorio, con la version que la identifica.
///
/// La version no es decorativa: es lo que permite saber si lo que tiene el
/// cliente delante es lo que el repositorio tiene guardado. Un cambio de
/// configuracion incrementa la version, y dos configuraciones con la misma
/// version son la misma configuracion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryConfig {
    /// La version de esta configuracion.
    pub version: u64,
    /// Donde viven los blobs de este repositorio.
    pub digest_scope: DigestScope,
    /// Los ajustes propios del formato.
    ///
    /// Un objeto vacio es un objeto valido: un repositorio recien creado
    /// no tiene ajustes que aplicar.
    #[serde(default)]
    pub config_settings: Value,
}

impl RepositoryConfig {
    /// La configuracion por defecto: version 1, digests compartidos y sin
    /// ajustes.
    pub fn new(digest_scope: DigestScope) -> Self {
        Self {
            version: 1,
            digest_scope,
            config_settings: Value::Object(serde_json::Map::new()),
        }
    }

    /// Si los ajustes son un objeto y no otra cosa.
    ///
    /// Un `settings` que no es un objeto no tiene interpretacion posible, y
    /// aceptarlo seria aceptar una configuracion que nadie puede leer.
    pub fn has_readable_settings(&self) -> bool {
        self.config_settings.is_object()
    }
}

/// La identidad de un repositorio.
///
/// El HRN es el nombre canonico del recurso y no se deduce de nada: lo
/// declara quien crea el repositorio, y el catalogo lo conserva tal cual
/// para que el mismo HRN siga significando lo mismo despues de un
/// reinicio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryDescriptor {
    /// El nombre canonico del recurso (HRN v2).
    pub hrn: Hrn,
    /// La cuenta propietaria.
    pub account: String,
    /// La clave corta del repositorio dentro de la cuenta.
    pub repo_key: String,
    /// El formato del contenido.
    pub format: RepositoryFormat,
    /// Como se comporta.
    pub mode: RepositoryMode,
    /// La configuracion versionada.
    pub config: RepositoryConfig,
    /// Cuando se creo.
    pub created_at: DateTime<Utc>,
    /// Cuando se toco por ultima vez.
    pub updated_at: DateTime<Utc>,
}

/// El HRN canonico de un repositorio.
///
/// Se construye con la misma forma que cualquier otro recurso HRN v2, con
/// el tipo `repository` y la clave del repositorio como identificador.
pub fn repository_hrn(
    partition: &str,
    service: &str,
    region: &str,
    account: &str,
    repo_key: &str,
) -> Hrn {
    Hrn::with_region(
        partition.to_string(),
        service.to_string(),
        region.to_string(),
        account.to_string(),
        "repository".to_string(),
        repo_key.to_string(),
    )
}

/// Las coordenadas de una version dentro de un repositorio.
///
/// Son inmutables: una version publicada no cambia de nombre ni de
/// contenido. Si el contenido cambia, es otra version.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageCoordinates {
    /// El nombre del paquete.
    pub package_name: String,
    /// La version del paquete.
    pub package_version: String,
}

/// Una version publicada: el contenido con su nombre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageVersion {
    /// La cuenta propietaria.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// El nombre y la version.
    pub coordinates: PackageCoordinates,
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// Los bytes que tiene el contenido.
    pub size_bytes: u64,
    /// Cuando se publico.
    pub published_at: DateTime<Utc>,
}

/// Un asset: una ruta de un repositorio.
///
/// El asset es la posicion, no el contenido: apunta a una version y a su
/// digest. Mover el asset a otra version es una operacion del indice, y
/// no toca los bytes de ninguna de las dos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetRecord {
    /// La cuenta propietaria.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// La ruta dentro del repositorio.
    pub asset_path: String,
    /// La version a la que apunta.
    pub coordinates: PackageCoordinates,
    /// El digest del contenido al que apunta.
    pub digest: ContentDigest,
    /// Los bytes que tiene ese contenido.
    pub size_bytes: u64,
    /// Cuando se movio por ultima vez.
    pub updated_at: DateTime<Utc>,
}

/// Una referencia viva a un blob del CAS.
///
/// Es lo que impide que el blob se recoja: mientras exista esta fila, el
/// contenido tiene un dueño.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlobReference {
    /// La cuenta propietaria.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// La ruta que referencia el blob.
    pub asset_path: String,
    /// El digest del blob.
    pub digest: ContentDigest,
    /// Donde vive el blob.
    pub scope: DigestScope,
    /// Los bytes que tiene el blob.
    pub size_bytes: u64,
    /// Cuando se escribio la referencia.
    pub updated_at: DateTime<Utc>,
}

/// Una etiqueta mutable: un nombre que apunta a una version.
///
/// Mover la etiqueta cambia esta fila y nada mas. La version que queda
/// atras conserva su digest y sus bytes, y sigue siendo alcanzable por
/// sus coordenadas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutableTag {
    /// La cuenta propietaria.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// El nombre de la etiqueta.
    pub tag_name: String,
    /// La version a la que apunta ahora.
    pub coordinates: PackageCoordinates,
    /// Cuando se movio.
    pub updated_at: DateTime<Utc>,
}

/// Lo que se quiere publicar, en una sola operacion.
///
/// Publicar son cuatro cosas a la vez: la version, el asset, la referencia
/// al blob y las etiquetas. Van juntas porque un asset sin version no
/// resuelve, y una version sin asset no se puede descargar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Publication {
    /// El repositorio donde se publica.
    pub repository: String,
    /// La ruta dentro del repositorio.
    pub asset_path: String,
    /// El nombre y la version.
    pub coordinates: PackageCoordinates,
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// Los bytes que tiene el contenido.
    pub size_bytes: u64,
    /// Las etiquetas que nacen con esta publicacion.
    pub tags: Vec<String>,
}

/// Lo que la publicacion dejo.
///
/// `created` dice si las filas se crearon o si ya estaban ahi con el mismo
/// contenido. Republicar lo mismo no es un error ni una escritura: es lo
/// mismo, dicho dos veces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishOutcome {
    /// La version.
    pub version: PackageVersion,
    /// El asset.
    pub asset: AssetRecord,
    /// La referencia al blob.
    pub reference: BlobReference,
    /// Si las filas se crearon.
    pub created: bool,
}
