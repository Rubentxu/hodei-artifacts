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
//!Guess de lo que habia ahi.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hodei_evidence::ContentDigest;

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
            Self::Unavailable(reason) => {
                write!(formatter, "the store is not available: {reason}")
            }
        }
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
    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError>;
}

/// El almacen de contenido en un directorio.
///
/// El blob vive en `<directorio>/<ab>/<cd>/<digest>`: los dos primeros
/// caracteres reparten los ficheros en directorios, que es lo que hace que
/// un directorio con un millon de blobs no sea un problema del sistema de
/// ficheros.
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
        let hex = digest.value.clone();
        self.root.join(&hex[..2]).join(&hex[2..4]).join(&hex)
    }
}

#[async_trait::async_trait]
impl BlobStore for FileBlobStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        let path = self.the_path_of(&digest);
        if path.exists() {
            // El mismo contenido no se guarda dos veces.
            return Ok(digest);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }
        // Se escribe en un temporal y se renombra: un renombrado en el
        // mismo sistema de ficheros es atomico, de modo que un blob
        // nunca esta a medio escribir.
        let temporary = path.with_extension("tmp");
        std::fs::write(&temporary, bytes)
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        std::fs::rename(&temporary, &path)
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        Ok(digest)
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        let path = self.the_path_of(digest);
        let bytes = std::fs::read(&path).map_err(|_| BlobError::NotFound)?;
        // Lo que sale tiene que SER lo que se pidio.
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
        Ok(self.the_path_of(digest).exists())
    }
}

/// El almacen de contenido en memoria.
///
/// Es el que se usa en las pruebas y el modo fixture; la semántica es la
/// misma que la del almacen en disco, incluida la comprobacion del digest
/// al leer.
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

#[async_trait::async_trait]
impl BlobStore for InMemoryBlobStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        let mut blobs = self.blobs.lock().expect("el almacen no esta envenenado");
        // El mismo contenido no se guarda dos veces: la clave es el
        // digest.
        blobs.insert(digest.value.clone(), bytes.to_vec());
        Ok(digest)
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        let blobs = self.blobs.lock().expect("el almacen no esta envenenado");
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
        let blobs = self.blobs.lock().expect("el almacen no esta envenenado");
        Ok(blobs.contains_key(&digest.value.clone()))
    }
}
