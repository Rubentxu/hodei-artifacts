//! El almacen de contenido por su digest: el CAS.
//!
//! A partir de M9 Hodei **posee los bytes**, y la primera propiedad de un
//! almacen que los posee es que **el mismo contenido no se guarda dos
//! veces**: la identidad del contenido es su digest, y el digest es lo que
//! se guarda.
//!
//! Y la segunda es que lo que sale por el digest es **exactamente** lo que
//! entro: al leer, el digest se vuelve a calcular y se compara. Un almacen
//! que devuelve algo sin comprobar no es un almacen de contenido, es una
//! guess de lo que habia ahi.
//!
//! # Lo que cambia con el CAS de produccion
//!
//! El CAS de M9 era correcto en su garantia central pero no operable como
//! base de un producto que recibe bytes de clientes:
//!
//! - **Escribia el temporal con un nombre por digest.** Dos productores que
//!   suben el mismo contenido al mismo tiempo escribian en el **mismo**
//!   fichero temporal, y el renombrado del uno podia publicar los bytes
//!   del otro. Ahora cada escritura en curso tiene su **propio** fichero de
//!   staging, y el commit es lo unico que publica.
//! - **Bufferizaba el contenido entero.** `put(&[u8])` ya recibia el
//!   contenido en memoria. Ahora hay una sesion de escritura que acepta
//!   trozos: la memoria del proceso no crece con el tamano del artefacto.
//! - **No validaba lo que publicaba.** El commit comprueba el algoritmo, la
//!   longitud y el digest contra lo que el cliente declara, y si no
//!   coincide, el staging se elimina y **no queda ninguna referencia
//!   visible**.
//! - **No distinguia los fallos.** Un error de disco y un contenido que no
//!   esta salian los dos como `NotFound`. Ahora cada fallo tiene su error:
//!   lo que no esta, lo que esta roto y lo que no se pudo leer.
//! - **No sabia limparse.** Las escrituras que quedaron a medias al caerse
//!   un proceso se quedan en disco para siempre. Ahora hay staging con
//!   caducidad y una operacion que lo limpia.
//!
//! Y lo que **no** cambia es la garantia: el mismo contenido no se guarda
//! dos veces, y lo que sale por el digest es lo que entro.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use hodei_evidence::{ContentDigest, DigestAlgorithm};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::reclaim::{
    BlobReclaim, ConditionalBlob, ConditionalOutcome, Reclaimed, StoredBlob, VersionMarker,
};

/// El directorio, dentro de la raiz del almacen, donde viven las
/// escrituras en curso.
const STAGING_DIR: &str = "staging";

/// Por que el almacen de contenido no pudo hacer lo que se le pedia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobError {
    /// El contenido no esta.
    NotFound,
    /// El contenido leido no coincide con su digest.
    Corrupted {
        /// El digest que se pidio.
        expected: ContentDigest,
        /// El digest que tiene lo leido.
        found: ContentDigest,
    },
    /// El contenido no tiene la longitud que se esperaba.
    LengthMismatch {
        /// La longitud que se esperaba.
        expected: u64,
        /// La longitud que tiene el contenido.
        found: u64,
    },
    /// El contenido no es el que se declaraba.
    DigestMismatch {
        /// El digest que se declaraba.
        expected: ContentDigest,
        /// El digest que tiene el contenido.
        found: ContentDigest,
    },
    /// El algoritmo del digest no lo calcula este almacen.
    UnsupportedAlgorithm(String),
    /// La sesion de escritura ya no admite mas operaciones.
    SessionClosed,
    /// El almacen no esta disponible.
    Unavailable(String),
}

impl std::fmt::Display for BlobError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(formatter, "the content is not there"),
            Self::Corrupted { expected, found } => write!(
                formatter,
                "the content does not match its digest: {expected} was asked and {found} is there"
            ),
            Self::LengthMismatch { expected, found } => write!(
                formatter,
                "the content is {found} bytes long and {expected} were expected"
            ),
            Self::DigestMismatch { expected, found } => write!(
                formatter,
                "the content does not match what was declared: {expected} was declared and {found} is there"
            ),
            Self::UnsupportedAlgorithm(algorithm) => write!(
                formatter,
                "the store cannot compute digests with {algorithm}"
            ),
            Self::SessionClosed => {
                write!(formatter, "the write session is closed")
            }
            Self::Unavailable(reason) => {
                write!(formatter, "the store is not available: {reason}")
            }
        }
    }
}

/// Como se comporta una lectura.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPolicy {
    /// Se recalcula el digest y se compara con el pedido.
    ///
    /// Es la politica por defecto, y la unica que se puede usar para
    /// contenido de un cliente: lo que sale tiene que ser lo que entro.
    Verify,
    /// Se lee sin recalcular, para contenido ya verificado.
    ///
    /// Ahorra el recalculo, y **no** comprueba nada: usarla sobre
    /// contenido no verificado es devolver una guess.
    Trust,
}

/// Lo que un commit espera encontrar.
///
/// Un commit no publica "lo que ha entrado": publica **exactamente** lo
/// que el cliente declara. Si algo no coincide, no se publica nada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitExpectation {
    /// El digest del contenido esperado.
    pub digest: ContentDigest,
    /// La longitud del contenido esperado, en bytes.
    pub length: u64,
}

impl CommitExpectation {
    /// La expectativa para un contenido que ya se tiene en memoria.
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            digest: ContentDigest::of_bytes(bytes),
            length: bytes.len() as u64,
        }
    }
}

/// Una escritura en curso.
///
/// Los bytes entran a trozos y el commit publica el contenido entero o no
/// publica nada: **nunca** queda un blob a medio escribir.
#[async_trait::async_trait]
pub trait BlobWriteSession: Send {
    /// Anade un trozo al contenido en curso.
    async fn append(&mut self, chunk: &[u8]) -> Result<(), BlobError>;

    /// Publica el contenido si y solo si es exactamente el esperado.
    ///
    /// Si el algoritmo, la longitud o el digest no coinciden, el staging
    /// se elimina y el error lo dice: no queda ninguna referencia visible.
    async fn commit(&mut self, expectation: CommitExpectation) -> Result<ContentDigest, BlobError>;

    /// Descarta el contenido en curso.
    async fn abort(&mut self) -> Result<(), BlobError>;

    /// Cuantos bytes llevan escritos.
    fn bytes_written(&self) -> u64;

    /// El digest de lo que lleva escrito.
    ///
    /// Se calcula mientras pasan los trozos, y es el mismo digest que el
    /// commit va a comprobar: quien lo consulta no vuelve a calcularlo
    /// por su cuenta, porque dos calculos del mismo contenido son dos
    /// oportunidades de discrepar.
    fn the_digest(&self) -> ContentDigest;

    /// Donde se esta escribiendo, si el almacen tiene esa nocion.
    ///
    /// Sirve para comprobar que dos escrituras en curso **no** comparten
    /// fichero: es lo que garantiza que dos productores del mismo
    /// contenido no se pisen.
    fn the_staging_path(&self) -> Option<&Path>;

    /// El identificador **persistible** de este staging.
    ///
    /// Un registro de sesiones durable guarda este handle y, al
    /// reabrir la sesion en otro proceso, se lo devuelve a
    /// [`BlobStore::resume`] para que el almacén recupere la escritura
    /// exacta: la ruta del fichero en el almacen de ficheros, el par
    /// `key`/`upload_id` del multipart en el object store. `None` dice
    /// que este almacen no puede reabrir una escritura: sus sesiones
    /// mueren con el proceso, como siempre.
    fn the_staging_handle(&self) -> Option<String> {
        None
    }

    /// Si el digest de la sesion cubre **todos** los bytes del staging.
    ///
    /// Una sesion reabierta en frio puede no tener el hasher del
    /// contenido que ya esta en el almacén —es el caso del multipart
    /// reanudado, cuyos trozos ya subidos no vuelven a pasar por aqui—.
    /// Quien cierra no debe comparar un digest parcial contra lo
    /// declarado: el almacén comprueba el contenido entero al publicar,
    /// que es la promesa de siempre —no se publica nada que no cuadre—.
    fn the_digest_covers_all(&self) -> bool {
        true
    }

    /// Marca la escritura como **durable**: su staging sobrevive al
    /// Drop del mango.
    ///
    /// Un registro con journal puede quedarse sin mango local (cache
    /// desalojado, réplica cambiada) sin que eso signifique que nadie
    /// abandonó la subida: lo que decide el destino del staging es el
    /// journal —commit, abort explicito o barrido—, no el último Drop.
    fn mark_durable(&mut self) {}

    /// Suelta el mango local **sin borrar** el staging durable.
    ///
    /// Es la mano que cambia de réplica: el registro deja el almacén
    /// cerrado —y su bloqueo advisory libre para que otra réplica lo
    /// reclame— despues de cada peticion, cuando ya no queda nada por
    /// debajo del offset que la autoridad declaro
    /// ([`BlobWriteSession::flush_durable`]). Un mango soltado aqui no
    /// es un abandono: la autoridad sigue abierta y el reanudado retoma
    /// la escritura por su handle.
    fn detach(&mut self) -> Result<(), BlobError> {
        // Por defecto no hay staging durable que soltar: cerrar es
        // cerrar, y el Drop del almacén hace su limpieza de siempre.
        Ok(())
    }

    /// Los bytes que **ya estan** en el almacén, no en el buffer del
    /// mango.
    ///
    /// En un multipart, un trozo subido es durable y lo que queda en el
    /// buffer no: si el proceso muere, la autoridad del journal solo
    /// puede declarar lo que el almacén tiene. El registro usa esto para
    /// saber si **puede soltar** el mango sin perder bytes: solo cuando
    /// lo durable llega a lo escrito. Un journal avanza hasta aqui, nunca
    /// mas alla.
    fn the_durable_offset(&self) -> u64 {
        self.bytes_written()
    }

    /// Deja en el almacén lo que ya es durable y devuelve ese offset: lo
    /// que sobrevive a la muerte de este proceso.
    ///
    /// El journal solo puede avanzar hasta aqui. En el almacen de
    /// ficheros es el fichero sincronizado —todo lo escrito—. En el
    /// multipart son los trozos **confirmados** por el servidor, con los
    /// que estaban en vuelo ya recogidos: lo que queda en el buffer no
    /// salio del proceso, y el cliente lo reenvia contra el `Range` que
    /// la autoridad declara.
    async fn flush_durable(&mut self) -> Result<u64, BlobError> {
        Ok(self.bytes_written())
    }
}

/// El almacen de contenido.
#[async_trait::async_trait]
pub trait BlobStore: Send + Sync {
    /// Guarda el contenido y devuelve su digest.
    ///
    /// Guardar el mismo contenido dos veces no es un error ni guarda dos
    /// veces: el digest es la identidad, y lo que hay con ese digest ya
    /// esta.
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError>;

    /// Lee el contenido de un digest.
    ///
    /// Lo que se devuelve tiene **el digest pedido**, y el almacen lo
    /// comprueba antes de devolverlo.
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError>;

    /// Si el contenido esta.
    ///
    /// Esto dice si el contenido **esta**, no si esta **bien**: la
    /// integridad se comprueba con `verify` o con una lectura que
    /// verifica. Un almacen que usa esto para afirmar que sus bytes son
    /// los que dice su digest esta mintiendo.
    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError>;

    /// Abre una sesion de escritura en streaming.
    ///
    /// La sesion es la via para subir contenido que no cabe en memoria:
    /// los bytes van entrando a trozos y el commit publica el todo.
    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError>;

    /// Reabre una escritura en curso por el handle que dejo
    /// [`BlobWriteSession::the_staging_handle`].
    ///
    /// Es la via de un registro de sesiones **durable**: una sesion
    /// abierta en una réplica se reanuda en otra sin perder los bytes
    /// que ya estan en el almacén. El handle es opaco para el
    /// llamante —una ruta propia del almacen, un par
    /// `key`/`upload_id`— y el offset `written` es lo que el journal
    /// declaro: el almacén lo acepta como verdad **solo si lo que hay
    /// en el staging cuadra con él**; si no, el resume falla y el
    /// registro aborta la sesion, nunca la reanuda sobre bytes
    /// dudosos.
    ///
    /// El default dice que este almacen no reabre: una sesion suya
    /// muere con el proceso.
    async fn resume(
        &self,
        _handle: &str,
        _written: u64,
    ) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        Err(BlobError::Unavailable(
            "this store cannot reopen an in-progress write".to_owned(),
        ))
    }

    /// Lee el contenido aplicando una politica explicita.
    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError>;

    /// La longitud del contenido, sin leerlo.
    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError>;

    /// Lee un **tramo** del contenido.
    ///
    /// El tramo empieza en `offset` y tiene `length` bytes. Es la vía
    /// para servir una parte de un contenido sin traerse el contenido
    /// entero: un `Range` de HTTP no puede pedir un byte y obligar al
    /// almacén a leerlo todo.
    ///
    /// Si el contenido no llega a cubrir el tramo, el error es
    /// `LengthMismatch`: el almacén no devuelve un tramo corto como si
    /// fuera el pedido, porque un cliente que pide 100 bytes y recibe 40
    /// no puede saber que le faltan 60.
    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError>;

    /// Comprueba que el contenido sigue siendo el que su digest dice.
    ///
    /// Si el contenido no esta, el error es `NotFound`; si esta pero no
    /// corresponde a su digest, el error es `Corrupted`. Son fallos
    /// distintos y no se confunden.
    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError>;

    /// La vista de recuperacion de bytes, si el almacen la tiene.
    ///
    /// Un almacen que no sabe listar ni borrar con barrera no se puede
    /// recolectar: el recolector se niega en vez de adivinar.
    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        None
    }

    /// La vista de borrado condicional, si el almacen la tiene (HSP-192,
    /// AC2).
    ///
    /// Un almacen sin capacidad condicional segura puede aun ser
    /// recolectado en modo legacy; lo que no puede es participar de una
    /// pasada coordinada sobre la clave original sin cuarentena
    /// versionada. El recolector pregunta esto, no una bandera de
    /// configuracion.
    fn conditional(&self) -> Option<&dyn crate::reclaim::ConditionalBlob> {
        None
    }
}

/// El almacen de contenido en un directorio.
///
/// El blob vive en `<directorio>/<ab>/<cd>/<digest>`: los dos primeros
/// caracteres reparten los ficheros en directorios, que es lo que hace que
/// un directorio con un millon de blobs no sea un problema del sistema de
/// ficheros.
///
/// Y las escrituras en curso viven aparte, en
/// `<directorio>/staging/<id>.part`, con un nombre **propio** de cada
/// escritura: dos producers del mismo contenido nunca comparten fichero.
pub struct FileBlobStore {
    root: PathBuf,
    /// La barrera entre publicar un blob y recolectarlo.
    ///
    /// Los commits que publican o reutilizan un blob, y el borrado del
    /// recolector, pasan por aqui: ninguno ve a medias lo del otro.
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl FileBlobStore {
    /// El almacen sobre un directorio.
    ///
    /// El directorio se crea si no esta.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, BlobError> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        Ok(Self {
            root,
            gate: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// Donde vive un blob.
    pub fn the_path_of(&self, digest: &ContentDigest) -> PathBuf {
        blob_path(&self.root, digest)
    }

    /// El directorio donde viven las escrituras en curso.
    pub fn the_staging_dir(&self) -> PathBuf {
        self.root.join(STAGING_DIR)
    }

    /// Las escrituras en curso que hay ahora mismo.
    ///
    /// Solo las que son **de este almacen**: el staging es una convencion
    /// del directorio, y un fichero con otro nombre ahi no es una
    /// escritura en curso nuestra. Se listan con la edad que tienen, y un
    /// fichero cuya fecha no se puede leer **no** se lista: sin fecha no
    /// hay edad, y lo que no se sabe no se toca.
    pub fn staging_entries(&self) -> Result<Vec<StagingEntry>, BlobError> {
        let mut entries = Vec::new();
        let read = match std::fs::read_dir(self.the_staging_dir()) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        for entry in read {
            let entry = entry.map_err(|error| BlobError::Unavailable(error.to_string()))?;
            let name = entry.file_name();
            if !is_a_staging_file(&name.to_string_lossy()) {
                continue;
            }
            let metadata = entry
                .metadata()
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
            let Some(modified) = metadata.modified().ok() else {
                // Sin fecha no se puede decidir nada: el fichero se queda.
                continue;
            };
            let age = SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO);
            entries.push(StagingEntry {
                path: entry.path(),
                length: metadata.len(),
                age,
            });
        }
        Ok(entries)
    }

    /// Elimina las escrituras en curso mas antiguas que `max_age`.
    ///
    /// Es la operacion de recuperacion: un proceso que se cae a mitad de
    /// una subida deja su staging en disco, y ese staging no se publica
    /// nunca. Con la caducidad, se limpia solo.
    ///
    /// Lo que decide no es la edad, sino el **bloqueo**: mientras una
    /// escritura vive, su fichero esta bloqueado por el proceso que
    /// escribe, y un fichero bloqueado no se borra. Por eso la edad es el
    /// margen de prudencia y no el criterio: un fichero viejo de una
    /// subida que lleva horas sin escribir sigue siendo de una subida
    /// viva, y quitarselo de debajo rompe la subida.
    ///
    /// Un fichero que no se pudo borrar **no** es un motivo para que el
    /// almacen no sirva: se avisa y se sigue. Lo que no puede borrar es un
    /// problema de disco, no un motivo para no arrancar.
    pub fn clean_expired_staging(&self, max_age: Duration) -> Result<StagingCleanup, BlobError> {
        let mut cleanup = StagingCleanup::default();
        for entry in self.staging_entries()? {
            if entry.age < max_age {
                continue;
            }
            // El bloqueo dice si alguien esta escribiendo este fichero:
            // si lo esta, es de una subida viva y no se toca.
            let file = match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&entry.path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => match std::fs::OpenOptions::new().read(true).open(&entry.path) {
                    Ok(file) => file,
                    Err(error) => {
                        cleanup
                            .failed
                            .push((entry.path.display().to_string(), error.to_string()));
                        continue;
                    }
                },
            };
            match file.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => {
                    cleanup.alive += 1;
                    continue;
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    cleanup
                        .failed
                        .push((entry.path.display().to_string(), error.to_string()));
                    continue;
                }
            }
            match std::fs::remove_file(&entry.path) {
                Ok(()) => cleanup.removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    cleanup
                        .failed
                        .push((entry.path.display().to_string(), error.to_string()));
                }
            }
        }
        Ok(cleanup)
    }
}

/// Si el nombre es el de una escritura en curso de este almacen.
///
/// El patron lo pone `new_session_id`: `<pid>-<nanos:x>-<counter:x>.part`.
/// Un fichero con otro nombre en el directorio del staging no es de este
/// almacen: no se lista, no se borra y no se cuenta.
fn is_a_staging_file(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".part") else {
        return false;
    };
    let mut parts = stem.split('-');
    let (Some(pid), Some(nanos), Some(counter), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !nanos.is_empty()
        && nanos.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !counter.is_empty()
        && counter.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Lo que paso al limpiar el staging.
///
/// La limpieza **no** falla: un fichero que no se pudo borrar se cuenta y
/// se avisa, y el almacen sigue sirviendo. Lo que se necesita para
/// contarlo es esto.
///
/// Compartido por los dos backends: para el de ficheros el identificador
/// es la ruta; para el de objetos, la clave. Ambas se muestran igual.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StagingCleanup {
    /// Cuantos restos se eliminaron.
    pub removed: usize,
    /// Cuantos restos habia que estan escribiendo ahora mismo.
    ///
    /// Son de subidas vivas: no se tocan.
    pub alive: usize,
    /// Los restos que no se pudieron borrar, con su motivo.
    pub failed: Vec<(String, String)>,
}

/// Una escritura en curso en el almacen de fichero.
///
/// El staging se borra solo si la sesion se abandona sin commitear: el
/// `Drop` lo limpia, de modo que un error, un `?` o un panic no dejan
/// basura en disco.
pub struct FileBlobWriteSession {
    root: PathBuf,
    gate: Arc<tokio::sync::Mutex<()>>,
    staging_path: PathBuf,
    file: Option<tokio::fs::File>,
    hasher: Sha256,
    written: u64,
    open: bool,
    /// El journal del registro declaro la sesion durable: el Drop del
    /// mango **no** toca el staging. Su destino lo decide el journal.
    durable: bool,
    /// Si el hasher local cubre el staging entero. Un mango reabierto en
    /// frio no volvio a ver los bytes que ya estaban en el fichero: su
    /// digest local es parcial, y el commit comprueba el contenido
    /// completo antes de publicar.
    covers_all: bool,
}

impl FileBlobWriteSession {
    /// El fichero de staging de esta escritura.
    pub fn the_staging_path(&self) -> &Path {
        &self.staging_path
    }

    /// El mango sobre un fichero de staging ya existente, en append.
    ///
    /// El resume de un journal pasa por aqui: se reclama el bloqueo del
    /// fichero —el `try_lock` del `begin`— y se recorta al offset que la
    /// autoridad declaro. Sin el bloqueo, la subida sigue escribiendo en
    /// otra réplica y este resume falla: no hay dos escritores del mismo
    /// staging.
    ///
    /// Un fichero **mas largo** que lo declarado no es un error: son los
    /// bytes que un escritor escribio y no alcanzo a registrar antes de
    /// morir. Se recortan —nunca se publican por delante de la
    /// autoridad—, y la subida sigue desde el offset declarado. Mas
    /// corto si lo seria: la autoridad no puede deber bytes al disco.
    ///
    /// El hasher NO se recorre aqui: un resume puede darse en cada
    /// peticion y recalcular el fichero entero seria pagar el doble. La
    /// sesion reabierta se marca como **parcial** y el commit comprueba
    /// el digest releyendo el staging —la verificacion es la misma, se
    /// paga una sola vez, al publicar—.
    fn reopen(
        root: PathBuf,
        gate: Arc<tokio::sync::Mutex<()>>,
        staging_path: PathBuf,
        declared: u64,
    ) -> Result<Self, BlobError> {
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&staging_path)
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        if let Err(error) = file.try_lock() {
            // El bloqueo es la prueba de que el dueno anterior murio:
            // un staging que otra escritura tiene tomado NO se reabre.
            return Err(BlobError::Unavailable(match error {
                std::fs::TryLockError::WouldBlock => {
                    "the staging file is being written by another process".to_owned()
                }
                std::fs::TryLockError::Error(error) => error.to_string(),
            }));
        }
        let length = file
            .metadata()
            .map_err(|error| BlobError::Unavailable(error.to_string()))?
            .len();
        if length < declared {
            let _ = file.unlock();
            return Err(BlobError::Unavailable(format!(
                "the staging file holds {length} bytes and the journal declared {declared}"
            )));
        }
        if length > declared {
            // Lo que la autoridad no registro no se publica: se recorta
            // al offset declarado. Son los bytes de un escritor muerto
            // —vivo no seria, el bloqueo lo impide— que no llego a
            // registrar su avance, o un duplicado que el CAS ya revento.
            // El cliente los reenvia contra el `Range` declarado.
            file.set_len(declared)
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }
        // El hasher no se recalcula aqui: un resume puede darse en cada
        // peticion y releer el fichero entero seria pagarlo dos veces.
        // La sesion reabierta se marca como **parcial** —`covers_all`
        // abajo— y el commit comprueba el digest contra el staging
        // completo, que es la misma verificacion honesta, una sola vez,
        // al publicar.
        let file = tokio::fs::File::from_std(file);
        Ok(Self {
            root,
            gate,
            staging_path,
            file: Some(file),
            hasher: Sha256::new(),
            written: declared,
            open: true,
            durable: true,
            covers_all: false,
        })
    }

    /// Cierra la sesion y borra el staging, sin publicar nada.
    fn discard(&mut self) {
        self.open = false;
        self.file = None;
        let _ = std::fs::remove_file(&self.staging_path);
    }
}

impl Drop for FileBlobWriteSession {
    fn drop(&mut self) {
        if self.open && !self.durable {
            self.discard();
        }
    }
}

#[async_trait::async_trait]
impl BlobWriteSession for FileBlobWriteSession {
    async fn append(&mut self, chunk: &[u8]) -> Result<(), BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        let file = self.file.as_mut().ok_or(BlobError::SessionClosed)?;
        file.write_all(chunk)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        self.hasher.update(chunk);
        self.written += chunk.len() as u64;
        Ok(())
    }

    async fn commit(&mut self, expectation: CommitExpectation) -> Result<ContentDigest, BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        // El algoritmo tiene que ser uno que este almacen sepa calcular:
        // si no, no se puede ni verificar lo que se ha escrito.
        if expectation.digest.algorithm != DigestAlgorithm::Sha256 {
            self.discard();
            return Err(BlobError::UnsupportedAlgorithm(
                expectation.digest.algorithm.to_string(),
            ));
        }

        // Lo que se publica esta en disco, no en el buffer del sistema.
        {
            let file = self.file.as_mut().ok_or(BlobError::SessionClosed)?;
            file.flush()
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
            file.sync_all()
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }

        // La comprobacion honesta del digest. Cuando el mango paso por
        // todos los bytes, el hasher local es la verdad y no hay que
        // volver a leer. Cuando la sesion se reablo en frio —otro
        // proceso escribio los primeros bytes—, el hasher local NO los
        // cubre: comparar ese digest parcial contra lo declarado daria
        // un `DigestMismatch` falso y abortaria una subida buena. Se
        // recalcula sobre el staging completo, una sola vez, al publicar.
        let found = if self.covers_all {
            ContentDigest::sha256(hex_of(&self.hasher.finalize_reset()))
        } else {
            let bytes = tokio::fs::read(&self.staging_path)
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
            ContentDigest::of_bytes(&bytes)
        };

        // La longitud se comprueba contra la declarada.
        if self.written != expectation.length {
            self.discard();
            return Err(BlobError::LengthMismatch {
                expected: expectation.length,
                found: self.written,
            });
        }
        // Y el digest, contra el declarado.
        if found != expectation.digest {
            self.discard();
            return Err(BlobError::DigestMismatch {
                expected: expectation.digest,
                found,
            });
        }

        let target = blob_path(&self.root, &found);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }

        // Desde aqui hasta publicar, el recolector no puede borrar nada:
        // lo que se mira (el blob esta) sigue siendo cierto al actuar.
        let gate = Arc::clone(&self.gate);
        let _barrier = gate.lock().await;

        // El mismo contenido no se guarda dos veces: si el blob ya esta,
        // el staging se tira y se devuelve el mismo digest. Y se renueva
        // su fecha: reutilizar un blob es lo que le dice al recolector
        // que no es huerfano aunque el catalogo todavia no lo vea.
        if target.exists() {
            touch(&target)?;
            self.discard();
            return Ok(found);
        }

        // El renombrado es atomico: un lector ve el blob entero o no ve
        // nada. Nunca ve bytes a medio escribir.
        tokio::fs::rename(&self.staging_path, &target)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        // El fichero conserva la fecha de su ultimo trozo, que puede ser
        // anterior a la gracia si la sesion estuvo parada: la fecha de
        // publicacion es la de ahora.
        touch(&target)?;
        // Y el directorio se sincroniza, para que el renombrado sobreviva
        // a un corte de luz.
        if let Some(parent) = target.parent()
            && let Ok(directory) = tokio::fs::File::open(parent).await
        {
            let _ = directory.sync_all().await;
        }

        self.open = false;
        self.file = None;
        Ok(found)
    }

    async fn abort(&mut self) -> Result<(), BlobError> {
        if !self.open {
            return Ok(());
        }
        let outcome = std::fs::remove_file(&self.staging_path);
        self.open = false;
        self.file = None;
        match outcome {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    fn bytes_written(&self) -> u64 {
        self.written
    }

    fn the_digest(&self) -> ContentDigest {
        // El hasher se clona para no consumirlo: la sesion puede seguir
        // escribiendo, y el digest de lo escrito hasta ahora tiene que
        // seguir siendo el mismo despues del siguiente trozo.
        ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()))
    }

    fn the_staging_path(&self) -> Option<&Path> {
        Some(&self.staging_path)
    }

    /// El handle persistible: la ruta del fichero de staging.
    ///
    /// Otro proceso que comparta el disco puede reclamar la escritura
    /// exacta por esta ruta, y la marca del fichero —el `try_lock`—
    /// dice si el dueño anterior sigue vivo: no hay dos escritores de
    /// un mismo staging.
    fn the_staging_handle(&self) -> Option<String> {
        Some(self.staging_path.display().to_string())
    }

    /// El journal toma la propiedad del staging: el Drop del mango ya
    /// no lo borra, su ciclo de vida lo decide el journal.
    fn mark_durable(&mut self) {
        self.durable = true;
    }

    /// Suelta el fichero **sin borrarlo**: cerrar el descriptor libera el
    /// bloqueo advisory, y la autoridad del journal decide cuando el
    /// staging dejo de tener dueño. Es la mano que cambia de réplica.
    fn detach(&mut self) -> Result<(), BlobError> {
        self.open = false;
        self.file = None;
        Ok(())
    }

    /// Un mango reabierto en frio no volvio a pasar los primeros bytes
    /// por el hasher: su digest local es parcial y el commit comprueba
    /// el fichero completo.
    fn the_digest_covers_all(&self) -> bool {
        self.covers_all
    }

    /// El fichero de staging es durable byte a byte: soltar el mango —o
    /// que muera el proceso— no pierde bytes, y la autoridad puede
    /// declarar todo lo escrito. Lo que queda por debajo es la pagina
    /// del sistema: se fuerza antes de declarar.
    async fn flush_durable(&mut self) -> Result<u64, BlobError> {
        let file = self.file.as_mut().ok_or(BlobError::SessionClosed)?;
        file.flush()
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        file.sync_all()
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        Ok(self.written)
    }
}

#[async_trait::async_trait]
impl BlobStore for FileBlobStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        let length = bytes.len() as u64;

        // El mismo contenido no se guarda dos veces. Si el blob ya esta,
        // se comprueba al menos que no sea un resto truncado: la
        // verificacion completa del contenido guardado es tarea del
        // scrub, no de cada escritura.
        let gate = Arc::clone(&self.gate);
        let barrier = gate.lock().await;
        if self.exists(&digest).await.unwrap_or(false) {
            match self.length(&digest).await {
                Ok(stored) if stored == length => {
                    touch(&self.the_path_of(&digest))?;
                    return Ok(digest);
                }
                Ok(stored) => {
                    return Err(BlobError::LengthMismatch {
                        expected: length,
                        found: stored,
                    });
                }
                Err(_) => { /* se sigue por el camino normal */ }
            }
        }

        // La barrera se suelta antes de escribir: el commit la toma por
        // su cuenta, y escribir los bytes no necesita excluir a nadie.
        drop(barrier);
        let mut session = self.begin().await?;
        session.append(bytes).await?;
        session
            .commit(CommitExpectation {
                digest: digest.clone(),
                length,
            })
            .await
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await
    }

    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        Some(self)
    }

    fn conditional(&self) -> Option<&dyn ConditionalBlob> {
        Some(self)
    }

    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        match tokio::fs::metadata(self.the_path_of(digest)).await {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        let staging_dir = self.the_staging_dir();
        tokio::fs::create_dir_all(&staging_dir)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        // El nombre es **propio** de esta escritura: dos productores del
        // mismo contenido escriben en ficheros distintos.
        let staging_path = staging_dir.join(format!("{}.part", new_session_id()));
        let file = tokio::fs::File::create(&staging_path)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        // El fichero se bloquea mientras se escribe: el bloqueo es lo que
        // dice que esta escritura **viva**, y lo que impide que la
        // limpieza de otro proceso se lo lleve de debajo. Sin esto, un
        // arranque que encuentra el fichero viejo se lo lleva y la subida
        // que llevaba minutos escribiendo falla al publicar.
        let std_file = match file.try_into_std() {
            Ok(std_file) => std_file,
            Err(file) => file.into_std().await,
        };
        std_file
            .try_lock()
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        let file = tokio::fs::File::from_std(std_file);
        Ok(Box::new(FileBlobWriteSession {
            root: self.root.clone(),
            gate: Arc::clone(&self.gate),
            staging_path,
            file: Some(file),
            hasher: Sha256::new(),
            written: 0,
            open: true,
            durable: false,
            covers_all: true,
        }))
    }

    /// Reabrir una escritura en curso a partir del handle que guardo
    /// un journal: la ruta del fichero de staging.
    ///
    /// Solo una réplica puede tener el fichero tomado —el bloqueo
    /// advisory del `begin`—. Un resume que llega despues de la muerte
    /// del dueno anterior reclava el bloqueo y sigue escribiendo donde
    /// lo dejo; uno que llega mientras el dueno vive, falla.
    async fn resume(
        &self,
        handle: &str,
        written: u64,
    ) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        let path = std::path::Path::new(handle);
        // El handle debe ser un staging de este almacen, no una ruta
        // arbitraria: se rechaza todo lo que no cuelgue del directorio
        // de staging propio.
        let staging_dir = tokio::fs::canonicalize(self.the_staging_dir())
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        let canonical = tokio::fs::canonicalize(path)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        if !canonical.starts_with(&staging_dir)
            || canonical.extension() != Some(std::ffi::OsStr::new("part"))
        {
            return Err(BlobError::Unavailable(
                "the session handle is not a staging file of this store".to_owned(),
            ));
        }
        let session = FileBlobWriteSession::reopen(
            self.root.clone(),
            Arc::clone(&self.gate),
            canonical,
            written,
        )?;
        Ok(Box::new(session))
    }

    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        let bytes = match tokio::fs::read(self.the_path_of(digest)).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(BlobError::NotFound);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        if policy == ReadPolicy::Verify {
            let found = ContentDigest::of_bytes(&bytes);
            if found != *digest {
                return Err(BlobError::Corrupted {
                    expected: digest.clone(),
                    found,
                });
            }
        }
        Ok(bytes)
    }

    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        match tokio::fs::metadata(self.the_path_of(digest)).await {
            Ok(metadata) => Ok(metadata.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(BlobError::NotFound),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let path = self.the_path_of(digest);
        let mut file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(BlobError::NotFound);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        // El tramo se pide en el sitio: se salta lo que no se quiere y se
        // leen solo los bytes pedidos. Un almacén que leyera el fichero
        // entero para devolver 100 bytes de un gigabyte estaria
        // obedeciendo al cliente a costa de todos los demas.
        if offset > 0 {
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }
        // El buffer se reserva con lo que **hay**, no con lo que pide el
        // llamante: un tramo que no cabe en el fichero no se reserva,
        // se dice que no existe. Pedir un tramo de un gigabyte de un
        // fichero de un kilobyte no puede tumbar el proceso por un
        // `vec![0; length]`.
        let size = file
            .metadata()
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?
            .len();
        let available = size.saturating_sub(offset);
        if offset > size || length > available {
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: available,
            });
        }
        let mut bytes = vec![0u8; usize::try_from(length).unwrap_or(0)];
        // Se lee hasta llenar el buffer o hasta el final del fichero: una
        // sola lectura puede devolver menos de lo pedido sin que falte
        // nada, y un tramo a medias sería un tramo que no es el que se
        // pidió.
        let mut read = 0usize;
        while read < bytes.len() {
            let chunk = file
                .read(&mut bytes[read..])
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
            if chunk == 0 {
                break;
            }
            read += chunk;
        }
        if read as u64 != length {
            // El contenido es más corto de lo pedido: el tramo no existe.
            // Se dice cuanto se leyó, para que el que pidió sepa que le
            // faltan bytes.
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: read as u64,
            });
        }
        Ok(bytes)
    }

    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await.map(|_| ())
    }
}

/// El almacen de contenido en memoria.
///
/// Es el que se usa en las pruebas y el modo fixture; la semántica es la
/// misma que la del almacen en disco, incluida la comprobacion del digest
/// al leer.
///
/// La diferencia es que **guarda los bytes en memoria**, y eso es
/// inherente a lo que es: sirve para fixtures y pruebas, no para artefactos
/// de cliente.
#[derive(Debug, Clone, Default)]
pub struct InMemoryBlobStore {
    blobs: Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>>,
}

impl InMemoryBlobStore {
    /// El almacen, vacio.
    pub fn empty() -> Self {
        Self::default()
    }
}

/// Una escritura en curso en el almacen en memoria.
///
/// Acumula los bytes porque es lo que hace un almacen en memoria; la
/// garantia de integridad es la misma.
///
/// Y el commit **guarda**: el contenido entra en el almacen con el mismo
/// digest que el del almacen en disco. Una sesion que valida y no guarda
/// seria un digest de nada.
pub struct InMemoryBlobWriteSession {
    blobs: Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>>,
    bytes: Vec<u8>,
    hasher: Sha256,
    open: bool,
}

#[async_trait::async_trait]
impl BlobWriteSession for InMemoryBlobWriteSession {
    async fn append(&mut self, chunk: &[u8]) -> Result<(), BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        self.bytes.extend_from_slice(chunk);
        self.hasher.update(chunk);
        Ok(())
    }

    async fn commit(&mut self, expectation: CommitExpectation) -> Result<ContentDigest, BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        self.open = false;
        if expectation.digest.algorithm != DigestAlgorithm::Sha256 {
            return Err(BlobError::UnsupportedAlgorithm(
                expectation.digest.algorithm.to_string(),
            ));
        }
        let length = self.bytes.len() as u64;
        if length != expectation.length {
            return Err(BlobError::LengthMismatch {
                expected: expectation.length,
                found: length,
            });
        }
        let found = ContentDigest::sha256(hex_of(&self.hasher.finalize_reset()));
        if found != expectation.digest {
            return Err(BlobError::DigestMismatch {
                expected: expectation.digest,
                found,
            });
        }
        // Y lo que se ha validado se guarda: el mismo contenido, con el
        // mismo digest, guardado una sola vez.
        let mut blobs = self.blobs.lock().expect("el almacén no está envenenado");
        blobs.insert(found.value.clone(), std::mem::take(&mut self.bytes));
        Ok(found)
    }

    async fn abort(&mut self) -> Result<(), BlobError> {
        self.open = false;
        self.bytes.clear();
        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn the_digest(&self) -> ContentDigest {
        ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()))
    }

    fn the_staging_path(&self) -> Option<&Path> {
        None
    }
}

#[async_trait::async_trait]
impl BlobStore for InMemoryBlobStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        let mut blobs = self.blobs.lock().expect("el almacén no está envenenado");
        // El mismo contenido no se guarda dos veces: la clave es el
        // digest.
        blobs.insert(digest.value.clone(), bytes.to_vec());
        Ok(digest)
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        let bytes = blobs
            .get(&digest.value.clone())
            .ok_or(BlobError::NotFound)?
            .clone();
        let found = ContentDigest::of_bytes(&bytes);
        if found != *digest {
            return Err(BlobError::Corrupted {
                expected: digest.clone(),
                found,
            });
        }
        Ok(bytes)
    }

    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        Ok(blobs.contains_key(&digest.value.clone()))
    }

    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        Ok(Box::new(InMemoryBlobWriteSession {
            blobs: Arc::clone(&self.blobs),
            bytes: Vec::new(),
            hasher: Sha256::new(),
            open: true,
        }))
    }

    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        let bytes = blobs
            .get(&digest.value.clone())
            .ok_or(BlobError::NotFound)?
            .clone();
        if policy == ReadPolicy::Verify {
            let found = ContentDigest::of_bytes(&bytes);
            if found != *digest {
                return Err(BlobError::Corrupted {
                    expected: digest.clone(),
                    found,
                });
            }
        }
        Ok(bytes)
    }

    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        let bytes = blobs
            .get(&digest.value.clone())
            .ok_or(BlobError::NotFound)?;
        Ok(bytes.len() as u64)
    }

    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        let bytes = blobs
            .get(&digest.value.clone())
            .ok_or(BlobError::NotFound)?;
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let end = start.saturating_add(usize::try_from(length).unwrap_or(usize::MAX));
        if start > bytes.len() || end > bytes.len() {
            // El contenido es más corto de lo pedido: el tramo no existe.
            let found = bytes.len().saturating_sub(start) as u64;
            return Err(BlobError::LengthMismatch {
                expected: length,
                found,
            });
        }
        Ok(bytes[start..end].to_vec())
    }

    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await.map(|_| ())
    }
}

/// Una escritura en curso que ha quedado en disco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagingEntry {
    /// Donde esta el fichero de staging.
    pub path: PathBuf,
    /// Cuantos bytes tiene escritos.
    pub length: u64,
    /// Cuanto tiempo lleva escrito.
    pub age: Duration,
}

/// Donde vive un blob, dentro de una raiz.
fn blob_path(root: &Path, digest: &ContentDigest) -> PathBuf {
    let hex = &digest.value;
    root.join(&hex[..2]).join(&hex[2..4]).join(hex)
}

/// Un identificador unico para una escritura en curso.
///
/// Combina el proceso, el reloj y un contador, para que dos escrituras
/// **del mismo** contenido, y hasta de procesos distintos, nunca
/// compartan fichero de staging.
fn new_session_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos:x}-{counter:x}", std::process::id())
}

/// El hexadecimal de los bytes.
fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Renueva la fecha de un blob: "se uso ahora".
///
/// Es la marca que lee el recolector. Si no se puede escribir, el error
/// sube: reutilizar un blob sin dejar la marca es dejar al recolector
/// libre para borrarlo.
fn touch(path: &Path) -> Result<(), BlobError> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| BlobError::Unavailable(error.to_string()))?;
    file.set_modified(SystemTime::now())
        .map_err(|error| BlobError::Unavailable(error.to_string()))
}

/// El digest que dice la ruta de un blob, si la ruta es la de uno.
///
/// Un blob es `<root>/<ab>/<cd>/<64 hex>`, con `ab` y `cd` los dos pares
/// de caracteres iniciales del nombre. Cualquier otra cosa no es de este
/// almacen y no se lista.
fn digest_of_blob_file(prefix_a: &str, prefix_b: &str, name: &str) -> Option<ContentDigest> {
    let valid = name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !valid || &name[..2] != prefix_a || &name[2..4] != prefix_b {
        return None;
    }
    Some(ContentDigest::sha256(name))
}

#[async_trait::async_trait]
impl BlobReclaim for FileBlobStore {
    async fn list_blobs(&self) -> Result<Vec<StoredBlob>, BlobError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let unavailable = |error: std::io::Error| BlobError::Unavailable(error.to_string());
            let mut blobs = Vec::new();
            for first in std::fs::read_dir(&root).map_err(unavailable)? {
                let first = first.map_err(unavailable)?;
                let first_name = first.file_name().to_string_lossy().into_owned();
                if first_name.len() != 2 || !first.file_type().map_err(unavailable)?.is_dir() {
                    continue;
                }
                for second in std::fs::read_dir(first.path()).map_err(unavailable)? {
                    let second = second.map_err(unavailable)?;
                    let second_name = second.file_name().to_string_lossy().into_owned();
                    if second_name.len() != 2 || !second.file_type().map_err(unavailable)?.is_dir()
                    {
                        continue;
                    }
                    for file in std::fs::read_dir(second.path()).map_err(unavailable)? {
                        let file = file.map_err(unavailable)?;
                        let name = file.file_name().to_string_lossy().into_owned();
                        // Primero la forma del nombre: un objeto de
                        // cuarentena (HSP-192) comparte shard pero lleva
                        // sufijo, y lo que no es un digest exacto no es
                        // de este almacen: no se lista ni se borra.
                        if name.len() != 64
                            || !name
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        {
                            continue;
                        }
                        let Some(digest) = digest_of_blob_file(&first_name, &second_name, &name)
                        else {
                            continue;
                        };
                        let metadata = match file.metadata() {
                            Ok(metadata) if metadata.is_file() => metadata,
                            Ok(_) => continue,
                            // Se borro mientras se listaba: ya no esta.
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                continue;
                            }
                            Err(error) => return Err(unavailable(error)),
                        };
                        // Sin fecha no hay edad, y sin edad no se puede
                        // decidir nada: es un fallo, no un blob joven.
                        let modified = metadata.modified().map_err(unavailable)?;
                        blobs.push(StoredBlob {
                            digest,
                            length: metadata.len(),
                            age: SystemTime::now()
                                .duration_since(modified)
                                .unwrap_or(Duration::ZERO),
                        });
                    }
                }
            }
            Ok(blobs)
        })
        .await
        .map_err(|error| BlobError::Unavailable(error.to_string()))?
    }

    async fn delete_if_older_than(
        &self,
        digest: &ContentDigest,
        grace: Duration,
    ) -> Result<Reclaimed, BlobError> {
        if digest.algorithm != DigestAlgorithm::Sha256 || digest.value.len() != 64 {
            return Err(BlobError::UnsupportedAlgorithm(
                digest.algorithm.to_string(),
            ));
        }
        let path = self.the_path_of(digest);
        let _barrier = self.gate.lock().await;
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Reclaimed::Gone);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        let modified = metadata
            .modified()
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO);
        if age < grace {
            return Ok(Reclaimed::Fresh);
        }
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                // El borrado se sincroniza con el directorio: un corte de
                // luz no resucita un blob que ya se dio por borrado.
                if let Some(parent) = path.parent()
                    && let Ok(directory) = tokio::fs::File::open(parent).await
                {
                    let _ = directory.sync_all().await;
                }
                Ok(Reclaimed::Deleted {
                    length: metadata.len(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Reclaimed::Gone),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }
}

/// La marca de una version de fichero: el mtime en nanos y el tamano.
///
/// Dos escrituras concurrentes de contenido distinto producen marcas
/// distintas; el rename no toca la marca, asi que una cuarentena o un
/// borrado condicional bajo la barrera es atomo-seguro.
fn file_marker(path_key: &str, metadata: &std::fs::Metadata) -> VersionMarker {
    let modified = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    VersionMarker {
        key: path_key.to_string(),
        marker: format!("mtime:{modified}:len:{}", metadata.len()),
        length: metadata.len(),
    }
}

#[async_trait::async_trait]
impl crate::reclaim::ConditionalBlob for FileBlobStore {
    fn supports_conditional_delete(&self) -> bool {
        // El rename sobre la barrera del almacen es atomico y no toca
        // la marca: en un POSIX local si se puede borrar una version
        // exacta sin pisar la reescritura de otro.
        true
    }

    fn quarantine_key_of(&self, digest: &ContentDigest, run_id: &str) -> String {
        self.root
            .join("quarantine")
            .join(run_id)
            .join(&digest.value)
            .to_string_lossy()
            .into_owned()
    }

    async fn head_marker(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<VersionMarker>, BlobError> {
        let path = self.the_path_of(digest);
        let key = path.to_string_lossy().into_owned();
        let _barrier = self.gate.lock().await;
        match tokio::fs::metadata(&path).await {
            Ok(metadata) => Ok(Some(file_marker(&key, &metadata))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    async fn delete_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
    ) -> Result<ConditionalOutcome, BlobError> {
        if digest.algorithm != DigestAlgorithm::Sha256 || digest.value.len() != 64 {
            return Err(BlobError::UnsupportedAlgorithm(
                digest.algorithm.to_string(),
            ));
        }
        let path = self.the_path_of(digest);
        let _barrier = self.gate.lock().await;
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConditionalOutcome::Gone);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        // La condicion se mira con la barrera tomada: sigue siendo
        // cierta cuando se borra debajo.
        let current = file_marker(&path.to_string_lossy(), &metadata);
        if current.marker != marker.marker {
            return Ok(ConditionalOutcome::NotApplied);
        }
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                if let Some(parent) = path.parent()
                    && let Ok(directory) = tokio::fs::File::open(parent).await
                {
                    let _ = directory.sync_all().await;
                }
                Ok(ConditionalOutcome::Applied {
                    length: metadata.len(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConditionalOutcome::Gone)
            }
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    async fn quarantine_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
        quarantine_key: &str,
    ) -> Result<ConditionalOutcome, BlobError> {
        if digest.algorithm != DigestAlgorithm::Sha256 || digest.value.len() != 64 {
            return Err(BlobError::UnsupportedAlgorithm(
                digest.algorithm.to_string(),
            ));
        }
        let path = self.the_path_of(digest);
        let quarantine = std::path::Path::new(quarantine_key);
        let _barrier = self.gate.lock().await;
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConditionalOutcome::Gone);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        let current = file_marker(&path.to_string_lossy(), &metadata);
        if current.marker != marker.marker {
            return Ok(ConditionalOutcome::NotApplied);
        }
        // La cuarentena es un nombre NUEVO, jamas la clave original:
        // un rename sobre la barrera deja la version marcada leible
        // bajo el nombre nuevo y libera la clave para la reescritura
        // del publisher sin carrera posible.
        if let Some(parent) = quarantine.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }
        match tokio::fs::rename(&path, quarantine).await {
            Ok(()) => Ok(ConditionalOutcome::Applied {
                length: metadata.len(),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConditionalOutcome::Gone)
            }
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El contenido de la prueba: bytes distinguibles, para que un tramo
    /// equivocado se note.
    fn the_bytes() -> Vec<u8> {
        (0u8..=255).collect()
    }

    #[tokio::test]
    async fn the_range_read_returns_exactly_the_bytes_asked_for() {
        let bytes = the_bytes();
        let digest = ContentDigest::of_bytes(&bytes);
        let store = InMemoryBlobStore::empty();
        store.put(&bytes).await.expect("the content is stored");

        let range = store
            .read_range(&digest, 10, 5)
            .await
            .expect("the range is readable");
        assert_eq!(range, bytes[10..15]);
    }

    #[tokio::test]
    async fn the_range_read_says_when_the_content_is_shorter() {
        let bytes = the_bytes();
        let digest = ContentDigest::of_bytes(&bytes);
        let store = InMemoryBlobStore::empty();
        store.put(&bytes).await.expect("the content is stored");

        // Un tramo que se sale del final no es un tramo corto: es un
        // tramo que no existe, y el error lo dice.
        let error = store
            .read_range(&digest, 250, 10)
            .await
            .expect_err("the range does not exist");
        assert_eq!(
            error,
            BlobError::LengthMismatch {
                expected: 10,
                found: 6
            }
        );
    }

    #[tokio::test]
    async fn the_range_read_of_a_content_that_is_not_there_is_not_found() {
        let store = InMemoryBlobStore::empty();
        let digest = ContentDigest::of_bytes(b"nothing");
        let error = store
            .read_range(&digest, 0, 1)
            .await
            .expect_err("the content is not there");
        assert_eq!(error, BlobError::NotFound);
    }

    #[tokio::test]
    async fn the_range_read_of_the_file_store_matches_the_memory_one() {
        let bytes = the_bytes();
        let digest = ContentDigest::of_bytes(&bytes);
        let directory = tempfile::tempdir().expect("a temp dir");
        let store = FileBlobStore::new(directory.path()).expect("the store is usable");
        store.put(&bytes).await.expect("the content is stored");

        let range = store
            .read_range(&digest, 10, 5)
            .await
            .expect("the range is readable");
        assert_eq!(range, bytes[10..15]);

        let error = store
            .read_range(&digest, 250, 10)
            .await
            .expect_err("the range does not exist");
        assert_eq!(
            error,
            BlobError::LengthMismatch {
                expected: 10,
                found: 6
            }
        );
    }

    /// HSP-192, AC2: la marca de version protege al que no fue visto.
    #[tokio::test]
    async fn a_conditional_delete_never_touches_an_unmarked_version() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let store = FileBlobStore::new(directory.path()).expect("the store is usable");
        let bytes = b"orphan bytes".to_vec();
        let digest = ContentDigest::of_bytes(&bytes);
        store.put(&bytes).await.expect("the content is stored");

        let marker = store
            .conditional()
            .expect("the file store is conditional")
            .head_marker(&digest)
            .await
            .expect("head works")
            .expect("the blob is there");

        // Otra version (la que un publisher pondria tras reescribir):
        // la condicion no se cumple y NO se toca nada.
        let stale = VersionMarker {
            marker: format!("{}-otro", marker.marker),
            ..marker.clone()
        };
        let outcome = store
            .conditional()
            .unwrap()
            .delete_if_untouched(&digest, &stale)
            .await
            .expect("the conditional delete answers");
        assert_eq!(outcome, ConditionalOutcome::NotApplied);
        assert!(
            store.exists(&digest).await.expect("exists"),
            "an unmarked version is never deleted"
        );

        // La version marcada si se borra.
        let outcome = store
            .conditional()
            .unwrap()
            .delete_if_untouched(&digest, &marker)
            .await
            .expect("the conditional delete answers");
        assert!(
            matches!(outcome, ConditionalOutcome::Applied { length } if length == 12),
            "the marked version is deleted: {outcome:?}"
        );
        assert!(!store.exists(&digest).await.expect("exists"));
    }

    /// HSP-192, AC2: la cuarentena es versionada —nombre nuevo, nunca
    /// reescritura de la clave original— y deja los bytes reconciliables.
    #[tokio::test]
    async fn a_quarantine_moves_the_marked_version_to_a_new_name() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let store = FileBlobStore::new(directory.path()).expect("the store is usable");
        let bytes = b"marked version".to_vec();
        let digest = ContentDigest::of_bytes(&bytes);
        store.put(&bytes).await.expect("the content is stored");

        let marker = store
            .conditional()
            .unwrap()
            .head_marker(&digest)
            .await
            .expect("head works")
            .expect("the blob is there");
        let quarantine_key = directory
            .path()
            .join("quarantine")
            .join(format!("{}.gc-run-1", digest.value))
            .to_string_lossy()
            .into_owned();

        let outcome = store
            .conditional()
            .unwrap()
            .quarantine_if_untouched(&digest, &marker, &quarantine_key)
            .await
            .expect("the quarantine answers");
        assert!(
            matches!(outcome, ConditionalOutcome::Applied { length } if length == 14),
            "the marked version is quarantined: {outcome:?}"
        );

        // La clave original se libero; los bytes viven bajo el nombre
        // nuevo, intactos.
        assert!(!store.exists(&digest).await.expect("exists"));
        let moved = std::fs::read(&quarantine_key).expect("the quarantined bytes are there");
        assert_eq!(moved, bytes);

        // Y list_blobs no los ve: lo que no reconoce como suyo no se
        // lista ni se borra. La cuarentena es de la reconciliacion, no
        // del censo.
        let listed = store
            .reclaimer()
            .unwrap()
            .list_blobs()
            .await
            .expect("the census works");
        assert!(
            listed.iter().all(|blob| blob.digest != digest),
            "a quarantined name is not a live blob"
        );
    }
}
