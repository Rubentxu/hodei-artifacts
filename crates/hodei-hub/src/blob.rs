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

    /// Donde se esta escribiendo, si el almacen tiene esa nocion.
    ///
    /// Sirve para comprobar que dos escrituras en curso **no** comparten
    /// fichero: es lo que garantiza que dos productores del mismo
    /// contenido no se pisen.
    fn the_staging_path(&self) -> Option<&Path>;
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
}

impl FileBlobStore {
    /// El almacen sobre un directorio.
    ///
    /// El directorio se crea si no esta.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, BlobError> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        Ok(Self { root })
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
                        cleanup.failed.push((entry.path.clone(), error.to_string()));
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
                    cleanup.failed.push((entry.path.clone(), error.to_string()));
                    continue;
                }
            }
            match std::fs::remove_file(&entry.path) {
                Ok(()) => cleanup.removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    cleanup.failed.push((entry.path.clone(), error.to_string()));
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StagingCleanup {
    /// Cuantos ficheros se eliminaron.
    pub removed: usize,
    /// Cuantos ficheros habia que estan escribiendo ahora mismo.
    ///
    /// Son de subidas vivas: no se tocan.
    pub alive: usize,
    /// Los ficheros que no se pudieron borrar, con su motivo.
    pub failed: Vec<(PathBuf, String)>,
}

/// Una escritura en curso en el almacen de fichero.
///
/// El staging se borra solo si la sesion se abandona sin commitear: el
/// `Drop` lo limpia, de modo que un error, un `?` o un panic no dejan
/// basura en disco.
pub struct FileBlobWriteSession {
    root: PathBuf,
    staging_path: PathBuf,
    file: Option<tokio::fs::File>,
    hasher: Sha256,
    written: u64,
    open: bool,
}

impl FileBlobWriteSession {
    /// El fichero de staging de esta escritura.
    pub fn the_staging_path(&self) -> &Path {
        &self.staging_path
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
        if self.open {
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

        let found = ContentDigest::sha256(hex_of(&self.hasher.finalize_reset()));

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

        // El mismo contenido no se guarda dos veces: si el blob ya esta,
        // el staging se tira y se devuelve el mismo digest.
        if target.exists() {
            self.discard();
            return Ok(found);
        }

        // El renombrado es atomico: un lector ve el blob entero o no ve
        // nada. Nunca ve bytes a medio escribir.
        tokio::fs::rename(&self.staging_path, &target)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
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

    fn the_staging_path(&self) -> Option<&Path> {
        Some(&self.staging_path)
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
        if self.exists(&digest).await.unwrap_or(false) {
            match self.length(&digest).await {
                Ok(stored) if stored == length => return Ok(digest),
                Ok(stored) => {
                    return Err(BlobError::LengthMismatch {
                        expected: length,
                        found: stored,
                    });
                }
                Err(_) => { /* se sigue por el camino normal */ }
            }
        }

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
            staging_path,
            file: Some(file),
            hasher: Sha256::new(),
            written: 0,
            open: true,
        }))
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
}
