//! El modelo del catálogo.
//!
//! Aquí se separan las cuatro cosas que un repositorio de artefactos
//! confunde con facilidad, y que este catálogo mantiene separadas:
//!
//! 1. **La identidad del repositorio**: cuenta, clave, formato, modo y
//!    configuración versionada.
//! 2. **La identidad del contenido**: el digest, que no depende de donde
//!    vive ni de quien lo publicó.
//! 3. **Las coordenadas del paquete**: el nombre y la versión, que son
//!    inmutables una vez publicadas.
//! 4. **La ruta del asset**: la posición dentro del repositorio, que es lo
//!    que el cliente pide y lo que puede apuntar a otra versión sin tocar
//!    los bytes.
//!
//! La separación no es estética. Es lo que permite que una etiqueta
//! mutable se mueva sin alterar un blob, y que dos cuentas usen el mismo
//! camino sin pisarse.

use chrono::{DateTime, Utc};
use hodei_evidence::ContentDigest;
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// El formato del contenido que guarda un repositorio.
///
/// El catálogo los conoce todos porque tiene que poder describir un
/// repositorio antes de que exista el adaptador que lo sirve. Un formato
/// que aún no se sirve no es un error: es un repositorio que se declara y
/// que espera su turno.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryFormat {
    /// Contenido opaco, served tal cual. Es el formato de laRaw 1.0.
    Raw,
    /// Imágenes OCI, con manifests y tags.
    Oci,
    /// Maven2, con `maven-metadata.xml` y layouts de grupo.
    Maven2,
    /// npm, con packuments, scopes y dist-tags.
    Npm,
    /// PyPI, con la Simple API.
    PyPi,
    /// Cargo, con el índice sparse.
    Cargo,
    /// NuGet v3, con service index y flat container.
    NuGet,
    /// Go Modules, con el protocolo GOPROXY.
    Go,
    /// Helm, con charts y repositorio de índices.
    Helm,
    /// APT, con índices y firmas.
    Apt,
    /// RPM, con creación de repositorios.
    Rpm,
}

impl RepositoryFormat {
    /// El nombre canónico del formato, el que va al cable y a la base.
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

    /// The format that says its canonical name.
    ///
    /// The name is the one that goes to the wire and to the database, and
    /// it is the same one [`RepositoryFormat::as_str`] returns: what is
    /// written is read, and what is read is written. A name that does not
    /// exist is not a format: it is a text nobody can interpret.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "raw" => Some(Self::Raw),
            "oci" => Some(Self::Oci),
            "maven2" => Some(Self::Maven2),
            "npm" => Some(Self::Npm),
            "pypi" => Some(Self::PyPi),
            "cargo" => Some(Self::Cargo),
            "nuget" => Some(Self::NuGet),
            "go" => Some(Self::Go),
            "helm" => Some(Self::Helm),
            "apt" => Some(Self::Apt),
            "rpm" => Some(Self::Rpm),
            _ => None,
        }
    }

    /// The modalities that this format serves in this release.
    ///
    /// A format is a way of storing content; whether the release knows to
    /// serve it in a given modality is a fact of the release, and it is
    /// written here, in one place, so that nobody has to answer the
    /// question twice and answer it differently.
    ///
    /// The raw surface, and the npm and PyPI ones that ride on it, serve
    /// the three modalities: what a client publishes, what a proxy brings
    /// from its upstream, and what a group composes of its members. The
    /// OCI surface and the cargo one serve only the hosted one for now:
    /// a push is a write, and a write lands where the client publishes.
    /// The proxy semantics of cargo need a client-verified upstream before
    /// they can be declared. The go and nuget surfaces serve the hosted
    /// one and the proxy one —their protocol documents were verified
    /// byte for byte— and neither assembles a group yet.
    ///
    /// The formats that are declared but not served yet —helm— have no
    /// modality at all: a repository of one of them can be declared, and
    /// it waits for its turn. Apt and rpm serve the hosted one: the
    /// distro clients install from what this hub publishes, and the proxy
    /// of a distro mirror is another protocol with another surface.
    pub fn serves_modes(&self) -> &'static [RepositoryMode] {
        match self {
            RepositoryFormat::Raw
            | RepositoryFormat::Maven2
            | RepositoryFormat::Npm
            | RepositoryFormat::PyPi => &[
                RepositoryMode::Hosted,
                RepositoryMode::Proxy,
                RepositoryMode::Group,
            ],
            RepositoryFormat::Oci | RepositoryFormat::Cargo => &[RepositoryMode::Hosted],
            RepositoryFormat::Apt | RepositoryFormat::Rpm => &[RepositoryMode::Hosted],
            RepositoryFormat::NuGet | RepositoryFormat::Go => {
                &[RepositoryMode::Hosted, RepositoryMode::Proxy]
            }
            RepositoryFormat::Helm => &[],
        }
    }

    /// Whether this format can be declared in that modality.
    ///
    /// The answer is the one of [`RepositoryFormat::serves_modes`], and it
    /// is asked in one place: a format that does not serve a modality
    /// cannot be declared in it.
    pub fn supports(&self, mode: &RepositoryMode) -> bool {
        self.serves_modes().contains(mode)
    }

    /// The formats that this release serves, in the order they are named.
    ///
    /// The formats that this release does not serve yet are not here, and
    /// they cannot be declared either: a repository of one of them would
    /// be a repository that nothing could serve. This list is what a
    /// client asks to know what it can create.
    pub fn served() -> &'static [RepositoryFormat] {
        &[
            RepositoryFormat::Raw,
            RepositoryFormat::Oci,
            RepositoryFormat::Maven2,
            RepositoryFormat::Npm,
            RepositoryFormat::PyPi,
            RepositoryFormat::Cargo,
            RepositoryFormat::Go,
            RepositoryFormat::NuGet,
            RepositoryFormat::Apt,
            RepositoryFormat::Rpm,
        ]
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
    /// El nombre canónico del modo, el que va al cable y a la base.
    pub fn as_str(&self) -> &'static str {
        match self {
            RepositoryMode::Hosted => "hosted",
            RepositoryMode::Proxy => "proxy",
            RepositoryMode::Group => "group",
        }
    }

    /// The mode that says its canonical name.
    ///
    /// The name is the one that goes to the wire and to the database, and
    /// it is the same one [`RepositoryMode::as_str`] returns. A name that
    /// does not exist is not a mode: it is a text nobody can interpret.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "hosted" => Some(Self::Hosted),
            "proxy" => Some(Self::Proxy),
            "group" => Some(Self::Group),
            _ => None,
        }
    }

    /// Si esta release sabe comportarse así.
    ///
    /// El catálogo sabe DESCRIBIR los tres modos, porque un repositorio se
    /// declara antes de existir; la release solo los SIRVE si sabe
    /// resolverlos. El `match` es exhaustivo a propósito: un modo nuevo
    /// obliga a decidir aquí si ya se sirve, y prometerlo sin cumplirlo sería
    /// una promesa falsa que se paga cuando alguien publica donde no hay
    /// nada.
    ///
    /// Un proxy resuelve contra el upstream que declara su configuración y
    /// lo que baja lo guarda como cualquier otro contenido. Un grupo resuelve
    /// en sus miembros, en orden, y no guarda nada propio.
    pub fn is_served_by_this_release(&self) -> bool {
        match self {
            RepositoryMode::Hosted | RepositoryMode::Proxy | RepositoryMode::Group => true,
        }
    }

    /// Si el repositorio guarda contenido propio (assets, versiones, blobs).
    ///
    /// Un grupo solo contiene una lista de miembros: lo que lee sale de
    /// ellos, nunca de filas suyas. Por eso ninguna escritura de contenido
    /// -- ni la de un cliente ni la caché de un proxy -- puede llegar a él.
    pub fn stores_content(&self) -> bool {
        matches!(self, RepositoryMode::Hosted | RepositoryMode::Proxy)
    }

    /// Si los clientes pueden publicar, etiquetar y mover aliases aquí.
    ///
    /// Solo un repositorio `Hosted` guarda lo que se publica en él. Un
    /// proxy escribe su propio contenido al resolver, pero ninguna petición
    /// de un cliente lo escribe ni lo mueve: lo que dice un proxy sale del
    /// upstream, no de quien pregunta.
    pub fn accepts_client_writes(&self) -> bool {
        matches!(self, RepositoryMode::Hosted)
    }
}

/// Donde vive el blob de un digest.
///
/// El digest identifica el contenido, pero no dice donde se guardan sus
/// bytes. Esa es una decision de la cuenta, y por eso es política y no
/// detalle de implementación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DigestScope {
    /// Un solo blob por digest, compartido por todas las cuentas.
    ///
    /// Dos cuentas que publican el mismo contenido comparten los bytes.
    /// Es lo que hace un registro eficiente, y también lo que hace que el
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
    /// El CAS guarda los digests SHA-256 por su valor hexadecimal, así que
    /// la clave global es ese valor. La forma particionada antepone la
    /// cuenta, y por eso las dos políticas no pueden colar el mismo blob
    /// en la misma ranura.
    pub fn blob_key(&self, account: &str, digest: &ContentDigest) -> String {
        match self {
            DigestScope::Global => digest.value.clone(),
            DigestScope::AccountPartitioned => format!("{account}/{}", digest.value),
        }
    }
}

/// Donde vive un blob concreto.
///
/// El digest dice **que** es el contenido; la ubicación dice **donde**
/// están sus bytes. Preguntar al almacén de bytes por el contenido
/// significa preguntar por las dos cosas: el mismo digest puede estar en
/// la ranura global o en la particionada de una cuenta, y quien busca solo
/// una de las dos encuentra un blob que no es el suyo, o no encuentra nada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobLocation {
    /// La cuenta a la que pertenece el contenido.
    pub account: String,
    /// El ámbito en el que vive.
    pub scope: DigestScope,
    /// El digest del contenido.
    pub digest: ContentDigest,
}

impl BlobLocation {
    /// La ubicación de un contenido en un ámbito.
    pub fn new(account: &str, scope: DigestScope, digest: ContentDigest) -> Self {
        Self {
            account: account.to_string(),
            scope,
            digest,
        }
    }

    /// La clave con la que el CAS guarda ese blob.
    pub fn blob_key(&self) -> String {
        self.scope.blob_key(&self.account, &self.digest)
    }
}

/// La configuración de un repositorio, con la versión que la identifica.
///
/// La versión no es decorativa: es lo que permite saber si lo que tiene el
/// cliente delante es lo que el repositorio tiene guardado. Un cambio de
/// configuración incrementa la versión, y dos configuraciones con la misma
/// versión son la misma configuración.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryConfig {
    /// La versión de esta configuración.
    pub version: u64,
    /// Donde viven los blobs de este repositorio.
    pub digest_scope: DigestScope,
    /// Los ajustes propios del formato.
    ///
    /// Un objeto vacío es un objeto válido: un repositorio recién creado
    /// no tiene ajustes que aplicar.
    #[serde(default)]
    pub config_settings: Value,
}

impl RepositoryConfig {
    /// La configuración por defecto: versión 1, digests compartidos y sin
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
    /// Un `settings` que no es un objeto no tiene interpretación posible, y
    /// aceptarlo sería aceptar una configuración que nadie puede leer.
    pub fn has_readable_settings(&self) -> bool {
        self.config_settings.is_object()
    }
}

/// La identidad de un repositorio.
///
/// El HRN es el nombre canónico del recurso y no se deduce de nada: lo
/// declara quien crea el repositorio, y el catálogo lo conserva tal cual
/// para que el mismo HRN siga significando lo mismo después de un
/// reinicio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryDescriptor {
    /// El nombre canónico del recurso (HRN v2).
    pub hrn: Hrn,
    /// La cuenta propietaria.
    pub account: String,
    /// La clave corta del repositorio dentro de la cuenta.
    pub repo_key: String,
    /// El formato del contenido.
    pub format: RepositoryFormat,
    /// Como se comporta.
    pub mode: RepositoryMode,
    /// La configuración versionada.
    pub config: RepositoryConfig,
    /// Cuando se creó.
    pub created_at: DateTime<Utc>,
    /// Cuando se tocó por última vez.
    pub updated_at: DateTime<Utc>,
}

/// El HRN canónico de un repositorio.
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

/// Las coordenadas de una versión dentro de un repositorio.
///
/// Son inmutables: una versión publicada no cambia de nombre ni de
/// contenido. Si el contenido cambia, es otra versión.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageCoordinates {
    /// El nombre del paquete.
    pub package_name: String,
    /// La versión del paquete.
    pub package_version: String,
}

/// Una versión publicada: el contenido con su nombre.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PackageVersion {
    /// La cuenta propietaria.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// El nombre y la versión.
    pub coordinates: PackageCoordinates,
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// Los bytes que tiene el contenido.
    pub size_bytes: u64,
    /// Cuando se publicó.
    pub published_at: DateTime<Utc>,
}

/// Un asset: una ruta de un repositorio.
///
/// El asset es la posición, no el contenido: apunta a una versión y a su
/// digest. Mover el asset a otra versión es una operación del índice, y
/// no toca los bytes de ninguna de las dos.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssetRecord {
    /// La cuenta propietaria.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// La ruta dentro del repositorio.
    pub asset_path: String,
    /// La versión a la que apunta.
    pub coordinates: PackageCoordinates,
    /// El digest del contenido al que apunta.
    pub digest: ContentDigest,
    /// Los bytes que tiene ese contenido.
    pub size_bytes: u64,
    /// El `subject` del manifiesto, si lo tiene.
    ///
    /// Es el digest del manifiesto al que este apunta: lo que hace que un
    /// manifiesto sea un **referrer** de otro. Un asset sin `subject` es
    /// un manifiesto del todo normal, y uno con `subject` cuelga de otro.
    pub subject: Option<ContentDigest>,
    /// Cuando se movió por última vez.
    pub updated_at: DateTime<Utc>,
}

/// Un referrer: un manifiesto que apunta a otro con `subject`.
///
/// Es la relación que la API de referrers del protocolo expone: los
/// manifiestos de una cuenta y un repositorio cuyo `subject` es un
/// digest. No dice nada sobre la **validez** de lo que el referrer
/// afirma: un referrer es un manifiesto que dice que cuelga de otro, y
/// quien juzgue lo que dice es el motor de confianza, no el catálogo.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferrerRecord {
    /// La cuenta propietaria.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// La ruta del referrer dentro del repositorio.
    pub asset_path: String,
    /// El digest del manifiesto del referrer.
    pub digest: ContentDigest,
    /// Los bytes que tiene el manifiesto del referrer.
    pub size_bytes: u64,
    /// El digest del `subject` al que apunta.
    pub subject: ContentDigest,
}

/// Una referencia viva a un blob del CAS.
///
/// La referencia dice que blob sirve **una ruta**: es la fila que se
/// mueve con el asset, para que la descarga y la recolección no
/// apontam a contenidos distintos.
///
/// El blob de una versión publicada lo protege su fila de versión, que es
/// inmutable y conserva el digest y la longitud. La referencia protege el
/// contenido que la ruta sirve **mientras lo sirva**: mover el asset
/// reescribe la referencia al digest nuevo, y la versión que se queda
/// atrás sigue declarada, con su digest, en su propia fila. Quien recoja
/// blobs tiene que tener en cuenta las dos cosas.
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
    /// En que canal vive el contenido en este repositorio.
    ///
    /// Lo escribe la **admisión**: un artefacto recién admitido esta en
    /// cuarentena, y salir de ella exige una decision de confianza. Y no
    /// es del blob: el blob es inmutable y lo que decide es la relación
    /// entre el repositorio y lo que guarda.
    pub trust_state: TrustState,
    /// Cuando se escribió la referencia.
    pub updated_at: DateTime<Utc>,
}

/// En que canal vive un artefacto dentro de un repositorio.
///
/// El estado **no** es del blob: el blob es inmutable y su digest es su
/// identidad, y el mismo contenido puede estar en cuarentena en una
/// cuenta y promovido en otra. El estado es de la **relación** entre el
/// repositorio y el contenido, que es lo que la referencia del catálogo
/// escribe.
///
/// Y el estado por defecto de un artefacto recién admitido es la
/// **cuarentena**: sin decision de confianza no hay promocion, y un
/// artefacto sin decision no ha dicho nada todavia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    /// El artefacto esta admitido, pero no promovido: esta en
    /// cuarentena.
    Quarantined,
    /// El artefacto ha sido promovido con una decision de confianza
    /// vigente.
    Trusted,
}

impl TrustState {
    /// El nombre canonico, tal y como va en la base.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Quarantined => "quarantined",
            Self::Trusted => "trusted",
        }
    }

    /// El estado que dice el nombre.
    ///
    /// Lo que no se conoce es cuarentena: un estado que no se puede
    /// leer no es un permiso, y un artefacto cuyo canal no se sabe no
    /// puede estar en el canal de confianza.
    pub fn of(name: &str) -> Self {
        match name {
            "trusted" => Self::Trusted,
            _ => Self::Quarantined,
        }
    }
}

/// Una etiqueta mutable: un nombre que apunta a una versión.
///
/// Mover la etiqueta cambia esta fila y nada más. La versión que queda
/// atrás conserva su digest y sus bytes, y sigue siendo alcanzable por
/// sus coordenadas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutableTag {
    /// La cuenta propietaria.
    pub account: String,
    /// El repositorio.
    pub repository: String,
    /// El nombre de la etiqueta.
    pub tag_name: String,
    /// La versión a la que apunta ahora.
    pub coordinates: PackageCoordinates,
    /// Cuando se movió.
    pub updated_at: DateTime<Utc>,
}

/// Lo que se quiere publicar, en una sola operación.
///
/// Publicar son cuatro cosas a la vez: la versión, el asset, la referencia
/// al blob y las etiquetas. Van juntas porque un asset sin versión no
/// resuelve, y una versión sin asset no se puede descargar.
///
/// La publicación lleva **quién** publica y con qué correlación, porque
/// publicar es una operación de alguien: sin eso, el rastro de lo que se
/// publicó no dice quién lo publicó. Lo que **no** lleva es la cuenta: el
/// tenant del evento de auditoría es la cuenta **verificada** del
/// principal, y una cuenta que declara el llamante no es una cuenta
/// verificada.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Publication {
    /// El repositorio donde se publica.
    pub repository: String,
    /// La ruta dentro del repositorio.
    pub asset_path: String,
    /// El nombre y la versión.
    pub coordinates: PackageCoordinates,
    /// El digest del contenido.
    pub digest: ContentDigest,
    /// Los bytes que tiene el contenido.
    pub size_bytes: u64,
    /// Las etiquetas que nacen con esta publicación.
    pub tags: Vec<String>,
    /// El actor que publica.
    ///
    /// El catálogo verifica la **cuenta** contra el autorizador, no el
    /// subject: por eso el actor lo declara quien publica, y por eso no se
    /// llama "verificado". En la capa HTTP el actor sale del principal
    /// verificado, así que allí sí lo es.
    pub actor: String,
    /// La correlación de la petición.
    pub correlation_id: String,
    /// Si la ruta puede apuntar a OTRA versión cuando ya apunta a una.
    ///
    /// El catálogo es, por defecto, **inmutable**: una ruta que ya apunta
    /// a una versión no se mueve sola, y publicar otro contenido en la
    /// misma ruta es un conflicto. Eso es lo correcto para un catálogo de
    /// paquetes, donde una versión publicada es un hecho que no se
    /// reescribe.
    ///
    /// Un servidor de contenido **crudo** es otra cosa: su ruta es la
    /// dirección del contenido, y reenviar un fichero a la misma
    /// dirección es reponer lo que hay. Por eso la sustitución es una
    /// **decisión explícita** y no un efecto secundario: quién publica
    /// dice si puede, y quién no lo dice, no puede.
    ///
    /// Lo que se sustituye es el **puntero**: la versión anterior no se
    /// borra ni se reescribe, sigue estando, y la ruta pasa a apuntar a
    /// la nueva.
    #[serde(default)]
    pub overwrite: bool,
    /// La condición que el cliente exige sobre lo que hay ahora.
    ///
    /// No es un adorno del llamante: es lo que hace que la escritura sea
    /// **condicional**, y el catálogo la comprueba **dentro de la misma
    /// transacción** que escribe las filas. Comprobarla antes, en la
    /// frontera, deja una ventana entre la comprobación y la escritura en
    /// la que otra publicación se cuela.
    ///
    /// Sin condición, la publicación es lo que es: una escritura.
    #[serde(default)]
    pub precondition: Option<Precondition>,
    /// El `subject` del manifiesto, si lo tiene.
    ///
    /// Es el digest del manifiesto al que este cuelga: lo que hace que un
    /// manifiesto sea un **referrer** de otro. Se escribe en la **misma**
    /// transacción que la publicación, y no en una segunda operación: un
    /// referrer visible a medias sería un referrer que existe cuando no
    /// existe, y la lista de referrers dejaría de ser la verdad.
    ///
    /// El `subject` **no** se comprueba contra el catálogo: el protocolo
    /// exige aceptar un manifiesto cuyo `subject` todavía no está, porque
    /// un cliente puede subir el referrer antes que el sujeto.
    #[serde(default)]
    pub subject: Option<ContentDigest>,
}

/// Lo que el cliente exige que haya en la ruta antes de escribir.
///
/// No es lo mismo que un conflicto: el conflicto dice que hay algo
/// distinto de lo que se publica, y la precondición dice que **lo que
/// hay** no es lo que el cliente esperaba. El cliente los trata distinto,
/// y por eso el catálogo los distingue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Precondition {
    /// Lo que hay en la ruta tiene que ser uno de estos contenidos.
    ///
    /// Es el `If-Match` de RFC 9110: una etiqueta, o una lista de
    /// etiquetas de las que tiene que coincidir una.
    Present(Vec<ContentDigest>),
    /// En la ruta no puede haber nada.
    ///
    /// Es el `If-None-Match: *` de RFC 9110.
    Absent,
    /// En la ruta tiene que haber algo.
    ///
    /// Es el `If-Match: *` de RFC 9110.
    Exists,
    /// Lo que hay en la ruta tiene que ser **otro** contenido.
    ///
    /// Es el `If-None-Match` con etiquetas sobre una escritura: "escribe,
    /// pero solo si lo que hay no es ninguno de estos".
    Other(Vec<ContentDigest>),
}

/// Lo que la publicación dejó.
///
/// `created` dice si las filas se crearon o si ya estaban ahí con el mismo
/// contenido. Republicar lo mismo no es un error ni una escritura: es lo
/// mismo, dicho dos veces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishOutcome {
    /// La versión.
    pub version: PackageVersion,
    /// El asset.
    pub asset: AssetRecord,
    /// La referencia al blob.
    pub reference: BlobReference,
    /// Si las filas se crearon.
    pub created: bool,
    /// Lo que pasó con la fila del asset.
    ///
    /// `created` dice si se escribió algo, y no dice **qué**: publicar
    /// por primera vez y reponer lo que hay escriben filas las dos, y
    /// para el cliente no es lo mismo. La disposición lo dice.
    pub disposition: AssetDisposition,
}

/// Lo que pasó con la fila del asset de una publicación.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetDisposition {
    /// La fila no existía, y se ha creado.
    Created,
    /// La fila existía, y se ha movido a otro contenido.
    ///
    /// La versión anterior no se borra ni se reescribe: lo que se mueve es
    /// el puntero.
    Replaced,
    /// La fila ya estaba con este contenido, y no se ha tocado.
    Unchanged,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_is_read_from_its_own_name() {
        let formats = [
            RepositoryFormat::Raw,
            RepositoryFormat::Oci,
            RepositoryFormat::Maven2,
            RepositoryFormat::Npm,
            RepositoryFormat::PyPi,
            RepositoryFormat::Cargo,
            RepositoryFormat::NuGet,
            RepositoryFormat::Go,
            RepositoryFormat::Helm,
            RepositoryFormat::Apt,
            RepositoryFormat::Rpm,
        ];
        for format in formats {
            assert_eq!(
                RepositoryFormat::parse(format.as_str()),
                Some(format),
                "{} does not read back",
                format.as_str()
            );
        }
        assert_eq!(RepositoryFormat::parse("zip"), None);
        assert_eq!(RepositoryFormat::parse(""), None);
    }

    #[test]
    fn every_mode_is_read_from_its_own_name() {
        let modes = [
            RepositoryMode::Hosted,
            RepositoryMode::Proxy,
            RepositoryMode::Group,
        ];
        for mode in modes {
            assert_eq!(
                RepositoryMode::parse(mode.as_str()),
                Some(mode),
                "{} does not read back",
                mode.as_str()
            );
        }
        assert_eq!(RepositoryMode::parse("mirror"), None);
        assert_eq!(RepositoryMode::parse("Hosted"), None);
    }
}
