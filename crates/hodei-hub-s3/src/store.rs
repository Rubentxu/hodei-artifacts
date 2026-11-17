//! El almacen de contenido sobre un Object Store que habla S3.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use aws_sdk_s3::Client;
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_hub::blob::{BlobError, BlobStore, BlobWriteSession, CommitExpectation, ReadPolicy};
use hodei_hub::reclaim::{BlobReclaim, Reclaimed, StoredBlob};
use sha2::{Digest, Sha256};

/// El prefijo bajo el que vive el contenido dentro del cubo.
const BLOB_PREFIX: &str = "blobs";

/// Lo que el constructor del almacen declaro mal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S3BlobConfigError {
    /// El tamano de trozo esta por debajo del minimo que S3 acepta
    /// para todo trozo que no sea el ultimo: cinco megabytes.
    PartTooSmall { given: usize, minimum: usize },
    /// No puede haber trozos en vuelo simultaneos.
    NoInflightParts,
}

/// Como se comporta este almacen.
#[derive(Clone)]
pub struct S3BlobConfig {
    /// El cubo donde vive el contenido.
    pub bucket: String,
    /// El tamano de los trozos de la subida multiple. La memoria en vuelo
    /// por sesion esta acotada por `part_size * max_inflight_parts`.
    pub part_size: usize,
    /// Cuantos trozos admite en vuelo una sesion antes de esperar.
    pub max_inflight_parts: usize,
}

impl S3BlobConfig {
    /// La configuracion con los valores por defecto del despliegue.
    pub fn new(bucket: impl Into<String>) -> Self {
        Self {
            bucket: bucket.into(),
            part_size: 8 * 1024 * 1024,
            max_inflight_parts: 4,
        }
    }

    /// El techo de memoria por sesion en curso, en bytes.
    pub fn the_memory_budget(&self) -> usize {
        self.part_size * self.max_inflight_parts
    }

    fn check(&self) -> Result<(), S3BlobConfigError> {
        const MINIMUM_PART: usize = 5 * 1024 * 1024;
        if self.part_size < MINIMUM_PART {
            return Err(S3BlobConfigError::PartTooSmall {
                given: self.part_size,
                minimum: MINIMUM_PART,
            });
        }
        if self.max_inflight_parts == 0 {
            return Err(S3BlobConfigError::NoInflightParts);
        }
        Ok(())
    }
}

/// El almacen de contenido en un Object Store S3-compatible.
///
/// El cliente llega construido de fuera, con la cadena de credenciales que
/// el despliegue eligio: el almacen no ve secretos y por eso no puede
/// filtrarlos. El `Debug` imprime la configuracion publica y el cubo, nunca
/// el cliente.
pub struct S3BlobStore {
    client: Client,
    config: S3BlobConfig,
    /// La barrera entre publicar un blob y recolectarlo, la misma
    /// garantida por el almacen de ficheros dentro del proceso.
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl std::fmt::Debug for S3BlobStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("S3BlobStore")
            .field("bucket", &self.config.bucket)
            .field("part_size", &self.config.part_size)
            .field("max_inflight_parts", &self.config.max_inflight_parts)
            .finish_non_exhaustive()
    }
}

impl S3BlobStore {
    /// El almacen sobre un cliente ya configurado.
    pub fn new(client: Client, config: S3BlobConfig) -> Result<Self, S3BlobConfigError> {
        config.check()?;
        Ok(Self {
            client,
            config,
            gate: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// La clave de un contenido.
    pub fn the_key_of(&self, digest: &ContentDigest) -> String {
        blob_key(digest)
    }

    /// El digest que dice una clave, si la clave es de un blob de este
    /// almacen. Cualquier otra cosa no se lista ni se borra.
    fn digest_of_key(key: &str) -> Option<ContentDigest> {
        let name = key
            .strip_prefix(&format!("{BLOB_PREFIX}/"))?
            .rsplit('/')
            .next()?;
        let valid = name.len() == 64
            && name
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            return None;
        }
        Some(ContentDigest::sha256(name))
    }

    /// El MD5 en base64 de un trozo, la condicion de integridad que S3
    /// comprueba en el servidor antes de dar el trozo por bueno.
    fn content_md5(chunk: &[u8]) -> String {
        use base64::Engine;
        use md5::Digest;
        base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(chunk).as_slice())
    }

    /// El ultimo modificado de S3, como edad desde ahora.
    fn age_of(last_modified: Option<&aws_smithy_types::DateTime>) -> Option<Duration> {
        let epoch = std::time::UNIX_EPOCH.elapsed().ok()?;
        let then = Duration::from_secs(last_modified?.secs().max(0) as u64);
        Some(epoch.saturating_sub(then))
    }

    /// El error de servicio con el que hay que hablar: lo que no esta es
    /// `NotFound`; lo demas es que el almacen no estuvo disponible.
    fn unavailable(operation: &str, detail: impl std::fmt::Display) -> BlobError {
        BlobError::Unavailable(format!("the object store failed to {operation}: {detail}"))
    }
}

/// Si el error de servicio dice que el contenido no esta.
fn is_not_found(error: &aws_sdk_s3::error::SdkError<impl ProvideErrorMetadata>) -> bool {
    matches!(
        ProvideErrorMetadata::code(error),
        Some("NoSuchKey") | Some("NotFound")
    )
}

#[async_trait::async_trait]
impl BlobStore for S3BlobStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        // En un CAS la clave es el digest del contenido que se envia, asi
        // que publicar dos veces el mismo contenido escribe los mismos
        // bytes en la misma clave: no es un error y no guarda dos veces.
        // La unica comprobacion que aporta el almacen de ficheros —que el
        // blob viejo no sea un resto truncado— la sustituye aqui el HEAD
        // que hace el llamante cuando le importa.
        self.client
            .put_object()
            .bucket(&self.config.bucket)
            .key(self.the_key_of(&digest))
            .content_md5(Self::content_md5(bytes))
            .body(ByteStream::from(bytes.to_vec()))
            .send()
            .await
            .map_err(|error| Self::unavailable("store the content", error))?;
        Ok(digest)
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await
    }

    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        match self.head(digest).await {
            Ok(_) => Ok(true),
            Err(BlobError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        // La escritura en curso nace como una subida multiple en una clave
        // de staging, fuera del espacio de los blobs: nada de lo que aqui
        // se sube es visible hasta el commit.
        let staging_key = new_staging_key();
        let created = self
            .client
            .create_multipart_upload()
            .bucket(&self.config.bucket)
            .key(&staging_key)
            .send()
            .await
            .map_err(|error| Self::unavailable("open the staging upload", error))?;
        let upload_id = created
            .upload_id()
            .map(|id| id.to_string())
            .unwrap_or_default();
        if upload_id.is_empty() {
            return Err(Self::unavailable(
                "open the staging upload",
                "the object store gave no upload id",
            ));
        }
        Ok(Box::new(S3BlobWriteSession {
            client: self.client.clone(),
            config: self.config.clone(),
            gate: Arc::clone(&self.gate),
            staging_key,
            upload_id,
            buffer: Vec::new(),
            pending: VecDeque::new(),
            completed: Vec::new(),
            part_number: 0,
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
        let bytes = self.read(digest, None).await?;
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
        let head = self.head(digest).await?;
        Ok(head.unwrap_or(0))
    }

    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        // La longitud total se consulta antes de pedir el tramo: S3
        // devuelve gustosamente un tramo truncado, y un cliente que pide
        // 100 bytes y recibe 40 no puede saber que le faltan 60. El
        // contrato del puerto no admite ese trato.
        let size = self.length(digest).await?;
        if length == 0 {
            return Ok(Vec::new());
        }
        if offset > size {
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: 0,
            });
        }
        let available = size - offset;
        if length > available {
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: available,
            });
        }
        let range = format!("bytes={}-{}", offset, offset + length - 1);
        let bytes = self.read(digest, Some(&range)).await?;
        if bytes.len() as u64 != length {
            // El objeto cambio de tamano entre el HEAD y el GET: el tramo
            // pedido ya no es el que hay. Se dice cuanto hay, no se
            // devuelve un tramo a medias.
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: bytes.len() as u64,
            });
        }
        Ok(bytes)
    }

    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await.map(|_| ())
    }

    fn reclaimer(&self) -> Option<&dyn hodei_hub::reclaim::BlobReclaim> {
        Some(self)
    }
}

impl S3BlobStore {
    /// La cabecera del blob: longitud y fecha, o `NotFound`.
    async fn head(&self, digest: &ContentDigest) -> Result<Option<u64>, BlobError> {
        match self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(self.the_key_of(digest))
            .send()
            .await
        {
            Ok(head) => Ok(head.content_length.map(|l| l.max(0) as u64)),
            Err(error) if is_not_found(&error) => Err(BlobError::NotFound),
            Err(error) => Err(Self::unavailable("look up the content", error)),
        }
    }

    /// Leer el contenido, entero o por tramo.
    async fn read(
        &self,
        digest: &ContentDigest,
        range: Option<&str>,
    ) -> Result<Vec<u8>, BlobError> {
        let mut request = self
            .client
            .get_object()
            .bucket(&self.config.bucket)
            .key(self.the_key_of(digest));
        if let Some(range) = range {
            request = request.range(range);
        }
        let got = match request.send().await {
            Ok(got) => got,
            Err(error) if is_not_found(&error) => return Err(BlobError::NotFound),
            Err(error) => return Err(Self::unavailable("read the content", error)),
        };
        let body = got
            .body
            .collect()
            .await
            .map_err(|error| Self::unavailable("stream the content", error))?;
        Ok(body.into_bytes().to_vec())
    }
}

/// La clave de un contenido dentro del prefijo de blobs.
fn blob_key(digest: &ContentDigest) -> String {
    let hex = &digest.value;
    format!("{BLOB_PREFIX}/{}/{}/{}", &hex[..2], &hex[2..4], hex)
}

/// La clave de una escritura en curso: vive en el prefijo del staging,
/// fuera del espacio de los blobs, hasta que el commit la publique.
fn new_staging_key() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!("staging/{}-{nanos:x}-{counter:x}", std::process::id())
}

/// El resultado de subir un trozo: su numero y el etag que devolvio el
/// almacén, con el que el commit lo reclama.
type UploadedPart = Result<CompletedPart, BlobError>;

/// Una escritura en curso sobre una subida multiple.
///
/// Los bytes se acumulan en un buffer hasta `part_size`; al llenarse, el
/// trozo sale volando hacia el almacén **con su Content-MD5**, para que la
/// condicion de integridad la compruebe el servidor y no nosotros al
/// final. En vuelo no puede haber mas de `max_inflight_parts` trozos: ese
/// es el presupuesto de memoria de la sesion, y excederlo es esperar, no
/// acumular.
///
/// El commit **no completa nada que no cuadre**: espera los trozos en
/// vuelo, comprueba largo y digest contra lo declarado y, si algo
/// discrepancy, aborta la subida entera. Un abort borra el staging del
/// almacén: no queda ninguna referencia visible, ni un objeto a medias.
pub struct S3BlobWriteSession {
    client: Client,
    config: S3BlobConfig,
    gate: Arc<tokio::sync::Mutex<()>>,
    staging_key: String,
    upload_id: String,
    buffer: Vec<u8>,
    pending: VecDeque<tokio::task::JoinHandle<UploadedPart>>,
    completed: Vec<CompletedPart>,
    part_number: i32,
    hasher: Sha256,
    written: u64,
    open: bool,
}

impl S3BlobWriteSession {
    /// Lanza el trozo completo que llena el buffer, esperando si ya hay
    /// demasiados en vuelo.
    async fn flush_part(&mut self, last: bool) -> Result<(), BlobError> {
        if self.buffer.is_empty() {
            return Ok(());
        }
        if !last && self.buffer.len() < self.config.part_size {
            return Ok(());
        }
        // El presupuesto de memoria se cumple esperando, no acumulando.
        while self.pending.len() >= self.config.max_inflight_parts {
            let oldest = self
                .pending
                .pop_front()
                .expect("habia trozos en vuelo para esperar");
            self.collect_part(tokio::time::timeout(UPLOAD_TIMEOUT, oldest).await)?;
        }
        let bytes = std::mem::take(&mut self.buffer);
        self.part_number += 1;
        let number = self.part_number;
        let client = self.client.clone();
        let bucket = self.config.bucket.clone();
        let key = self.staging_key.clone();
        let upload_id = self.upload_id.clone();
        let handle = tokio::spawn(async move {
            let md5 = S3BlobStore::content_md5(&bytes);
            let sent = client
                .upload_part()
                .bucket(&bucket)
                .key(&key)
                .upload_id(&upload_id)
                .part_number(number)
                .content_md5(md5)
                .body(ByteStream::from(bytes))
                .send()
                .await
                .map_err(|error| S3BlobStore::unavailable("upload a part", error))?;
            Ok(CompletedPart::builder()
                .part_number(number)
                .set_e_tag(sent.e_tag().map(|etag| etag.to_string()))
                .build())
        });
        self.pending.push_back(handle);
        Ok(())
    }

    /// Recoge un trozo terminado y lo apunta para el commit.
    fn collect_part(
        &mut self,
        joined: Result<Result<UploadedPart, tokio::task::JoinError>, tokio::time::error::Elapsed>,
    ) -> Result<(), BlobError> {
        let part = match joined {
            Ok(Ok(part)) => part?,
            Ok(Err(joined)) => {
                return Err(S3BlobStore::unavailable("join the part upload", joined));
            }
            Err(_elapsed) => {
                return Err(S3BlobStore::unavailable(
                    "upload a part",
                    "the object store did not answer within the timeout",
                ));
            }
        };
        self.completed.push(part);
        Ok(())
    }

    /// Descarta la escritura en curso: aborta la subida y no publica nada.
    async fn discard(&mut self) {
        self.open = false;
        self.buffer.clear();
        for handle in self.pending.drain(..) {
            handle.abort();
        }
        let client = self.client.clone();
        let bucket = self.config.bucket.clone();
        let key = self.staging_key.clone();
        let upload_id = self.upload_id.clone();
        let _ = tokio::spawn(async move {
            client
                .abort_multipart_upload()
                .bucket(&bucket)
                .key(&key)
                .upload_id(&upload_id)
                .send()
                .await
        })
        .await;
    }

    /// Publica el contenido validado bajo su clave de digest.
    ///
    /// S3 completa la subida multiple en la clave donde nacio —la de
    /// staging—, asi que publicar son dos pasos atomicos de servidor:
    /// completar, y copiar el resultado a la clave del digest. La copia
    /// es server-side: los bytes no vuelven a pasar por el proceso. Y el
    /// staging se borra justo despues: lo que no se borro lo caduca el
    /// ciclo de vida del cubo, nunca el recolector de blobs.
    async fn publish(&self, target: &str) -> Result<(), BlobError> {
        if self.completed.is_empty() {
            // Ni un byte: no hay nada que completar, y S3 no admite
            // subidas multiples vacias. El contenido vacio se publica
            // con un PUT normal, que es atomico por contrato.
            self.client
                .put_object()
                .bucket(&self.config.bucket)
                .key(target)
                .body(ByteStream::from_static(&[]))
                .send()
                .await
                .map_err(|error| S3BlobStore::unavailable("publish the empty content", error))?;
            return Ok(());
        }
        let mut parts = self.completed.clone();
        parts.sort_by_key(CompletedPart::part_number);
        self.client
            .complete_multipart_upload()
            .bucket(&self.config.bucket)
            .key(&self.staging_key)
            .upload_id(&self.upload_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .send()
            .await
            .map_err(|error| S3BlobStore::unavailable("complete the upload", error))?;
        let copied = if self.written > MAX_SIMPLE_COPY {
            self.copy_as_multipart(target).await
        } else {
            // La copia es server-side: los bytes no vuelven a pasar por
            // el proceso.
            self.client
                .copy_object()
                .bucket(&self.config.bucket)
                .key(target)
                .copy_source(format!("{}/{}", self.config.bucket, self.staging_key))
                .send()
                .await
                .map(|_| ())
                .map_err(|error| S3BlobStore::unavailable("publish the blob", error))
        };
        let staging_cleanup = self
            .client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(&self.staging_key)
            .send()
            .await
            .map_err(|error| S3BlobStore::unavailable("drop the staging object", error));
        copied?;
        staging_cleanup.map(|_| ())
    }

    /// Copiar un objeto de mas de cinco gigabytes: `CopyObject` no llega
    /// alli, asi que la copia se hace trozo a trozo, con ranges sobre la
    /// clave de staging. Es lo mismo que hace el desplegador maduro con
    /// s3-sr-copy y aqui no puede ser un paso externo: el commit es o todo
    /// o nada.
    async fn copy_as_multipart(&self, target: &str) -> Result<(), BlobError> {
        let created = self
            .client
            .create_multipart_upload()
            .bucket(&self.config.bucket)
            .key(target)
            .send()
            .await
            .map_err(|error| S3BlobStore::unavailable("open the copy upload", error))?;
        let upload_id = created
            .upload_id()
            .map(|id| id.to_string())
            .unwrap_or_default();
        if upload_id.is_empty() {
            return Err(S3BlobStore::unavailable(
                "open the copy upload",
                "the object store gave no upload id",
            ));
        }
        let source = format!("{}/{}", self.config.bucket, self.staging_key);
        let mut parts = Vec::new();
        let mut number = 0i32;
        let mut start = 0u64;
        while start < self.written {
            number += 1;
            let end = (start + self.config.part_size as u64 - 1).min(self.written - 1);
            let sent = self
                .client
                .upload_part_copy()
                .bucket(&self.config.bucket)
                .key(target)
                .copy_source(&source)
                .upload_id(&upload_id)
                .part_number(number)
                .copy_source_range(format!("bytes={start}-{end}"))
                .send()
                .await;
            match sent {
                Ok(sent) => parts.push(
                    CompletedPart::builder()
                        .part_number(number)
                        .set_e_tag(
                            sent.copy_part_result()
                                .and_then(|r| r.e_tag())
                                .map(|etag| etag.to_string()),
                        )
                        .build(),
                ),
                Err(error) => {
                    let abort = self
                        .client
                        .abort_multipart_upload()
                        .bucket(&self.config.bucket)
                        .key(target)
                        .upload_id(&upload_id)
                        .send()
                        .await;
                    drop(abort);
                    return Err(S3BlobStore::unavailable("copy a part", error));
                }
            }
            start = end + 1;
        }
        self.client
            .complete_multipart_upload()
            .bucket(&self.config.bucket)
            .key(target)
            .upload_id(&upload_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .send()
            .await
            .map_err(|error| S3BlobStore::unavailable("finish the copy", error))?;
        Ok(())
    }
}

/// Cuanto puede tardar un trozo antes de que la sesion lo de por perdido.
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);

/// Lo que `CopyObject` sirve de una sola vez: por encima, la publicacion
/// necesita una copia multiple. Cinco gigabytes, el maximo del servidor.
const MAX_SIMPLE_COPY: u64 = 5 * 1024 * 1024 * 1024;

#[async_trait::async_trait]
impl BlobWriteSession for S3BlobWriteSession {
    async fn append(&mut self, chunk: &[u8]) -> Result<(), BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        self.buffer.extend_from_slice(chunk);
        self.hasher.update(chunk);
        self.written += chunk.len() as u64;
        while self.buffer.len() >= self.config.part_size {
            let rest = self.buffer.split_off(self.config.part_size);
            self.flush_part(false).await?;
            self.buffer = rest;
        }
        Ok(())
    }

    async fn commit(&mut self, expectation: CommitExpectation) -> Result<ContentDigest, BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        if expectation.digest.algorithm != DigestAlgorithm::Sha256 {
            self.discard().await;
            return Err(BlobError::UnsupportedAlgorithm(
                expectation.digest.algorithm.to_string(),
            ));
        }
        // Lo ultimo que queda en el buffer es el trozo final, y el trozo
        // final es el unico que puede ser mas corto que el presupuesto.
        self.flush_part(true).await?;
        while let Some(handle) = self.pending.pop_front() {
            self.collect_part(tokio::time::timeout(UPLOAD_TIMEOUT, handle).await)?;
        }

        let found = ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()));
        if self.written != expectation.length {
            self.discard().await;
            return Err(BlobError::LengthMismatch {
                expected: expectation.length,
                found: self.written,
            });
        }
        if found != expectation.digest {
            self.discard().await;
            return Err(BlobError::DigestMismatch {
                expected: expectation.digest,
                found,
            });
        }

        // Desde aqui hasta publicar, el recolector no puede borrar nada.
        let barrier = self.gate.lock().await;
        let target = blob_key(&found);
        let outcome = self.publish(&target).await;
        drop(barrier);
        self.open = false;
        outcome?;
        Ok(found)
    }

    async fn abort(&mut self) -> Result<(), BlobError> {
        if !self.open {
            return Ok(());
        }
        self.open = false;
        self.buffer.clear();
        for handle in self.pending.drain(..) {
            handle.abort();
        }
        if self.completed.is_empty() && self.written == 0 {
            // Nada subido: tampoco hay nada que abortar en el almacén.
            return Ok(());
        }
        self.client
            .abort_multipart_upload()
            .bucket(&self.config.bucket)
            .key(&self.staging_key)
            .upload_id(&self.upload_id)
            .send()
            .await
            .map_err(|error| S3BlobStore::unavailable("abort the upload", error))?;
        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        self.written
    }

    fn the_digest(&self) -> ContentDigest {
        ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()))
    }

    fn the_staging_path(&self) -> Option<&std::path::Path> {
        None
    }
}

impl Drop for S3BlobWriteSession {
    fn drop(&mut self) {
        if !self.open {
            return;
        }
        // La sesion se abandono sin commitear ni abortar: el staging no
        // puede sobrevivir a quien lo abrio. El abort va al runtime en
        // curso; si no hay runtime, el ciclo de vida del cubo (o la
        // politica de ciclo de vida del staging) lo recoge.
        self.open = false;
        for handle in self.pending.drain(..) {
            handle.abort();
        }
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let client = self.client.clone();
            let bucket = self.config.bucket.clone();
            let key = self.staging_key.clone();
            let upload_id = self.upload_id.clone();
            handle.spawn(async move {
                let _ = client
                    .abort_multipart_upload()
                    .bucket(&bucket)
                    .key(&key)
                    .upload_id(&upload_id)
                    .send()
                    .await;
            });
        }
    }
}

#[async_trait::async_trait]
impl BlobReclaim for S3BlobStore {
    async fn list_blobs(&self) -> Result<Vec<StoredBlob>, BlobError> {
        let mut blobs = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.config.bucket)
                .prefix(format!("{BLOB_PREFIX}/"))
                .max_keys(1000);
            if let Some(token) = &token {
                request = request.continuation_token(token);
            }
            let page = request
                .send()
                .await
                .map_err(|error| Self::unavailable("list the blobs", error))?;
            for object in page.contents() {
                // Lo que no tiene forma de blob de este almacen no es de
                // este almacen: no se lista y, por tanto, no se borra.
                let Some(key) = object.key() else { continue };
                let Some(digest) = Self::digest_of_key(key) else {
                    continue;
                };
                // Sin fecha no hay edad, y sin edad no se decide nada:
                // un objeto sin Last-Modified no se ofrece al recolector.
                let Some(age) = Self::age_of(object.last_modified()) else {
                    continue;
                };
                blobs.push(StoredBlob {
                    digest,
                    length: object.size.unwrap_or(0).max(0) as u64,
                    age,
                });
            }
            if page.is_truncated() == Some(true) {
                token = page.next_continuation_token.map(|t| t.to_string());
                if token.is_none() {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(blobs)
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
        let key = self.the_key_of(digest);
        let _barrier = self.gate.lock().await;
        let head = match self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(head) => head,
            Err(error) if is_not_found(&error) => return Ok(Reclaimed::Gone),
            Err(error) => return Err(Self::unavailable("look up the blob", error)),
        };
        // La edad se mira con la barrera tomada: lo que se comprueba aqui
        // sigue siendo cierto cuando se borra abajo.
        let Some(age) = Self::age_of(head.last_modified()) else {
            return Err(Self::unavailable(
                "check the blob age",
                "the object store served no last-modified",
            ));
        };
        if age < grace {
            return Ok(Reclaimed::Fresh);
        }
        let length = head.content_length.unwrap_or(0).max(0) as u64;
        match self
            .client
            .delete_object()
            .bucket(&self.config.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => Ok(Reclaimed::Deleted { length }),
            Err(error) if is_not_found(&error) => Ok(Reclaimed::Gone),
            Err(error) => Err(Self::unavailable("delete the blob", error)),
        }
    }
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
