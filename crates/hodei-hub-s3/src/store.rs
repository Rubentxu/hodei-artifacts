//! El almacen de contenido sobre un Object Store que habla S3.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use aws_sdk_s3::Client;
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use hodei_evidence::{ContentDigest, DigestAlgorithm};
use hodei_hub::blob::{
    BlobError, BlobStore, BlobWriteSession, CommitExpectation, ReadPolicy, StagingCleanup,
};
use hodei_hub::reclaim::{BlobReclaim, Reclaimed, StoredBlob};
use sha2::{Digest, Sha256};

/// El prefijo bajo el que vive el contenido dentro del cubo.
const BLOB_PREFIX: &str = "blobs";

/// El prefijo bajo el que vive el staging: las subidas multiples en
/// curso y los restos que un fallo dejo entre el complete y la copia.
const STAGING_PREFIX: &str = "staging";

/// La clave bajo la que el almacen estampa el digest del objeto. El ETag
/// no sirve —en un multipart no es el sha256 del cuerpo—, y una publicacion
/// que no se distingue de un resto ajeno obliga a releer para deduplicar.
/// El sello es una ayuda para no reescribir lo que ya esta bien; lo que
/// manda, cuando hay que decidir en serio, son los bytes.
const DIGEST_METADATA: &str = "hodei-sha256";

/// Hasta donde merece la pena releer un preexistente sin sello para
/// comprobar si es el contenido: por encima de esto, la lectura entera
/// cuesta mas que la copia que se quiere evitar, y se repara escribiendo.
const MAX_VERIFY_ON_STAMP_MISS: u64 = 64 * 1024 * 1024;

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

    /// La cabecera cruda de una clave, con `None` si no esta. Es el HEAD
    /// que la decision de no-sobrescritura necesita, y distingue "no
    /// esta" de "no puedo mirar" —lo segundo no se maquilla de lo primero.
    async fn head_key(
        &self,
        key: &str,
    ) -> Result<Option<aws_sdk_s3::operation::head_object::HeadObjectOutput>, BlobError> {
        match self
            .client
            .head_object()
            .bucket(&self.config.bucket)
            .key(key)
            .send()
            .await
        {
            Ok(head) => Ok(Some(head)),
            Err(error) if is_not_found(&error) => Ok(None),
            Err(error) => Err(Self::unavailable("look up the content", error)),
        }
    }

    /// Que hacer cuando un PUT o una copia condicionada rechaza escribir
    /// porque la clave ya existe. Es el punto donde el CAS se juega su
    /// palabra: lo que hay ahi puede ser exactamente el contenido (una
    /// deduplicacion legitima, no se toca) o puede ser otra cosa (un
    /// resto truncado, un objeto plantado por fuera, corruption). Lo
    /// primero se informa; lo segundo **no se presenta como dedupe**:
    /// se repara escribiendo el contenido bueno por la via normal.
    ///
    /// La comprobacion empieza por el sello de metadatos —barata, un
    /// HEAD—, y solo si el preexistente no lleva sello (fue escrito por
    /// otra mano y su procedencia no consta) se leen los bytes, con el
    /// techo de `MAX_VERIFY_ON_STAMP_MISS`: por encima, releer cuesta
    /// mas que reescribir, y la reparacion por escritura sale gratis
    /// sea lo que sea lo que habia. Un preexistente verificado como
    /// bueno y sin sello se renueva con sello: la siguiente publicacion
    /// deduplica por HEAD, no por lectura.
    ///
    /// La precondicion de confianza del sello hay que decirla con todas
    /// las letras: **el sello vale porque lo estampa quien verifico
    /// antes de escribir**, y eso solo lo garantizan los almacenes de
    /// este crate. Un cubo compartido con manos que escribes metadata
    /// a su antojo —otro servicio, un `aws s3 cp` con `--metadata`, un
    /// operador con acceso— puede plantar un sello mentiroso y esta
    /// decision lo aceptaria sin releer. La topologia soportada es el
    /// cubo exclusivo de este servicio (la que Hodei despliega); si al
    /// gun dia un backend multiinquilino entra en el contrato, el sello
    /// pasa a ser una optimizacion sospechosa y esto exige lectura
    /// siempre —lo decide quien despliega, no un refactor.
    async fn settle_a_refused_write(
        &self,
        digest: &ContentDigest,
        bytes: Option<&[u8]>,
        repair: impl std::future::Future<Output = Result<(), BlobError>> + Send,
    ) -> Result<(), BlobError> {
        let key = self.the_key_of(digest);
        let Some(head) = self.head_key(&key).await? else {
            // El preexistente se fue entre el rechazo y el HEAD: la
            // ventana del concurrente. Reparar (escribir) es el camino
            // correcto —nadie ha publicado el contenido todavia.
            return repair.await;
        };
        match the_stamp_of(&head) {
            // Con sello propio, la identidad del contenido la garantiza
            // quien lo escribio verificandolo antes de sellar.
            Some(stamped) if stamped == *digest => return Ok(()),
            Some(_) => {
                // Lleva el sello de OTRO digest en la clave de este: la
                // clave y el contenido discrepan. Se repara escribiendo
                // lo que se envio; quien llame y quiera el veredicto de
                // integridad, que pregunte despues con `verify`.
                return repair.await;
            }
            None => {}
        }
        // Sin sello: no consta que sea lo que dice la clave.
        let length = head.content_length.unwrap_or(0).max(0) as u64;
        let cheap_to_check = length <= MAX_VERIFY_ON_STAMP_MISS;
        if let Some(expected) = bytes.filter(|_| cheap_to_check) {
            if self.read(digest, None).await?.as_slice() == expected {
                // Es el contenido. Se estampa sin mover un byte: la
                // copia sobre si misma con directiva REPLACE de
                // metadatos es la unica reescritura del objeto que no
                // necesita subir el cuerpo otra vez.
                self.client
                    .copy_object()
                    .bucket(&self.config.bucket)
                    .key(&key)
                    .copy_source(format!("{}/{}", self.config.bucket, key))
                    .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace)
                    .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
                    .send()
                    .await
                    .map_err(|error| Self::unavailable("stamp the verified blob", error))?;
                return Ok(());
            }
            return repair.await;
        }
        // O no tenemos los bytes (la via de la sesion) o el objeto es
        // grande: verificar es mas caro que reparar, y la reparacion es
        // segura —la autoridad del CAS es el digest de lo que se envia.
        repair.await
    }

    /// El mapa de metadatos que sella un objeto con su digest.
    fn the_stamp_map(digest: &ContentDigest) -> HashMap<String, String> {
        let mut items = HashMap::new();
        items.insert(DIGEST_METADATA.to_string(), digest.value.clone());
        items
    }

    /// Un PUT con la condicion que el backend ofrece. Con `only_if_absent`
    /// el servidor rechaza (412) si la clave ya existe —la escritura
    /// condicional medida en AC4—; sin ella es el PUT normal de siempre.
    /// En ambas formas el objeto sale sellado con su digest: la condicion
    /// de no-sobrescritura solo vale si despues hay con que probar que lo
    /// que hay es lo que se publico.
    ///
    /// El error viaja encajado: el `SdkError` del PUT pesa cientos de
    /// bytes, y esta funcion se llama en el camino caliente de cada
    /// publicacion —el Result ancho costaria mas moverlo que el error
    /// que describe. Quien solo mira el codigo (`is_precondition_failed`)
    /// ni lo nota.
    async fn conditional_put(
        &self,
        digest: &ContentDigest,
        bytes: &[u8],
        only_if_absent: bool,
    ) -> Result<
        (),
        Box<aws_sdk_s3::error::SdkError<aws_sdk_s3::operation::put_object::PutObjectError>>,
    > {
        let mut request = self
            .client
            .put_object()
            .bucket(&self.config.bucket)
            .key(self.the_key_of(digest))
            .content_md5(Self::content_md5(bytes))
            .set_metadata(Some(Self::the_stamp_map(digest)))
            .body(ByteStream::from(bytes.to_vec()));
        if only_if_absent {
            request = request.if_none_match("*");
        }
        request.send().await.map(|_| ()).map_err(Box::new)
    }

    /// El segado del staging: los restos que un fallo o un proceso
    /// matado dejaron entre el `create` y la publicacion, en el cubo
    /// S3. Es el equivalente del `clean_expired_staging` del almacen de
    /// ficheros, y cumple su misma palabra: **no toca una subida viva**.
    ///
    /// Aqui la prueba de vida es mas fuerte que el lock del fichero:
    /// una sesion abandona sin abortar dejo su `upload_id` en el
    /// almacén, y lo que la distingue de una subida en curso es que
    /// nadie escribe en ella. Se pide la fecha del ultimo trozo subido
    /// (y la del propio `create` si no hay trozos), y solo lo que lleva
    /// mas de `max_age` sin un byte nuevo es un resto: se aborta. El
    /// abort es el unico borrado —un multipart a medias no es un
    /// objeto, y sus trozos no se listan como tales; no hay nada que
    /// `list_blobs` pueda ver y el recolector pueda reclamar.
    ///
    /// Un fallo puntual abortando **no** es un fallo del segado: se
    /// cuenta y se sigue, igual que en el directorio, porque lo que no
    /// se pudo borrar hoy se vuelve a intentar en el arranque de
    /// manana. Y un `upload_id` sin fecha util tampoco se toca: sin
    /// reloj no hay mayoria de edad, y borrar a ciegas es peor que
    /// dejar.
    pub async fn clean_staging(&self, max_age: Duration) -> Result<StagingCleanup, BlobError> {
        let mut cleanup = StagingCleanup::default();
        let mut key_marker: Option<String> = None;
        let mut upload_marker: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_multipart_uploads()
                .bucket(&self.config.bucket)
                .prefix(format!("{STAGING_PREFIX}/"))
                .max_uploads(1000);
            if let Some(marker) = &key_marker {
                request = request.key_marker(marker);
            }
            if let Some(marker) = &upload_marker {
                request = request.upload_id_marker(marker);
            }
            let page = request
                .send()
                .await
                .map_err(|error| Self::unavailable("list the staging uploads", error))?;
            for upload in page.uploads() {
                let (Some(key), Some(upload_id)) = (upload.key(), upload.upload_id()) else {
                    continue;
                };
                // La clave tiene que ser de este almacen: `staging/<pid>-…`.
                // Cualquier otra cosa bajo el prefijo no es nuestra y no
                // se toca —la misma regla que `list_blobs` con las claves.
                if !is_a_staging_key(key) {
                    continue;
                }
                // La edad puede no poder pedirse —un 500 del servidor,
                // una subida que se cierra justo mientras la miramos.
                // La palabra del segado es que un fallo puntual **no**
                // aborta la pasada: se cuenta, como el abort que falla,
                // y se sigue con la siguiente subida. Perder una subida
                // vieja hoy es un inconveniente; no segar nada porque un
                // `ListParts` tuvo un mal día sería desobedecer el
                // contrato de esta función.
                let age = match self
                    .the_age_of_the_upload(key, upload_id, upload.initiated())
                    .await
                {
                    Ok(age) => age,
                    Err(error) => {
                        cleanup.failed.push((key.to_string(), error.to_string()));
                        continue;
                    }
                };
                let Some(age) = age else { continue };
                if age < max_age {
                    // Demasiado joven para ser un resto: puede estar
                    // escribiendo alguien ahora mismo.
                    cleanup.alive += 1;
                    continue;
                }
                match self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.config.bucket)
                    .key(key)
                    .upload_id(upload_id)
                    .send()
                    .await
                {
                    Ok(_) => cleanup.removed += 1,
                    // El upload se fue entre el listado y el abort:
                    // alguien (el dueno, o el Drop de su sesion) lo
                    // cerro. No es un fallo, y no se cuenta.
                    Err(error) if is_not_found(&error) => {}
                    Err(error) => cleanup.failed.push((key.to_string(), error.to_string())),
                }
            }
            if page.is_truncated() != Some(true) {
                break;
            }
            key_marker = page.next_key_marker.map(|value| value.to_string());
            upload_marker = page.next_upload_id_marker.map(|value| value.to_string());
            if key_marker.is_none() {
                break;
            }
        }
        // Los objetos de staging que quedaron entre el complete y la
        // copia de una publicacion interrumpida: un objeto normal bajo
        // el prefijo, ya sin upload_id, que nadie lista como blob y que
        // nadie va a leer nunca. Su fecha es `Last-Modified`.
        let mut token: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.config.bucket)
                .prefix(format!("{STAGING_PREFIX}/"))
                .max_keys(1000);
            if let Some(token) = &token {
                request = request.continuation_token(token.clone());
            }
            let page = request
                .send()
                .await
                .map_err(|error| Self::unavailable("list the staging objects", error))?;
            for object in page.contents() {
                let (Some(key), Some(age)) = (object.key(), Self::age_of(object.last_modified()))
                else {
                    continue;
                };
                if !is_a_staging_key(key) || age < max_age {
                    continue;
                }
                match self
                    .client
                    .delete_object()
                    .bucket(&self.config.bucket)
                    .key(key)
                    .send()
                    .await
                {
                    Ok(_) => cleanup.removed += 1,
                    Err(error) if is_not_found(&error) => {}
                    Err(error) => cleanup.failed.push((key.to_string(), error.to_string())),
                }
            }
            if page.is_truncated() != Some(true) {
                break;
            }
            token = page.next_continuation_token.map(|value| value.to_string());
            if token.is_none() {
                break;
            }
        }
        Ok(cleanup)
    }

    /// La edad de una subida: la mas reciente entre su creacion y su
    /// ultimo trozo. Una sesion viva actualiza el reloj con cada
    /// `upload_part`, y por eso una subida de horas de `create` pero de
    /// minutos de trozo no es un resto.
    async fn the_age_of_the_upload(
        &self,
        key: &str,
        upload_id: &str,
        initiated: Option<&aws_smithy_types::DateTime>,
    ) -> Result<Option<Duration>, BlobError> {
        let mut ages: Vec<Duration> = Self::age_of(initiated).into_iter().collect();
        let mut part_marker: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_parts()
                .bucket(&self.config.bucket)
                .key(key)
                .upload_id(upload_id)
                .max_parts(1000);
            if let Some(marker) = &part_marker {
                request = request.part_number_marker(marker.clone());
            }
            let page = match request.send().await {
                Ok(page) => page,
                // La subida se cerro entre el listado y la pregunta:
                // ya no existe, no es un resto que segar.
                Err(error) if is_not_found(&error) => return Ok(Some(Duration::ZERO)),
                Err(error) => {
                    return Err(Self::unavailable(
                        "list the parts of a staging upload",
                        error,
                    ));
                }
            };
            ages.extend(
                page.parts()
                    .iter()
                    .filter_map(|part| Self::age_of(part.last_modified())),
            );
            if page.is_truncated() != Some(true) {
                break;
            }
            part_marker = page.next_part_number_marker.map(|value| value.to_string());
            if part_marker.is_none() {
                break;
            }
        }
        // Sin ninguna fecha util, no se decide: la edad desconocida no
        // es mayoria de edad.
        Ok(ages.into_iter().min())
    }
}

/// Si el error de servicio dice que el contenido no esta.
fn is_not_found(error: &aws_sdk_s3::error::SdkError<impl ProvideErrorMetadata>) -> bool {
    matches!(
        ProvideErrorMetadata::code(error),
        Some("NoSuchKey") | Some("NotFound")
    )
}

/// Si el error de servicio es la precondicion de no-sobrescritura: el
/// objeto ya estaba cuando el PUT condicionado intento crearlo.
fn is_precondition_failed(error: &aws_sdk_s3::error::SdkError<impl ProvideErrorMetadata>) -> bool {
    matches!(
        ProvideErrorMetadata::code(error),
        Some("PreconditionFailed")
    )
}

/// Si la clave es una escritura en curso de este almacen.
///
/// El patron lo pone `new_staging_key`: `staging/<pid>-<nanos:x>-<counter:x>`,
/// el mismo del almacen de ficheros sin el `.part`. Una clave con otra
/// forma bajo el prefijo del staging no es de este almacen: no se lista,
/// no se aborta y no se cuenta —podria ser el staging de otra aplicacion
/// compartiendo el cubo, y borrar lo ajeno no entra en el contrato.
///
/// El discriminador es formal, no de propiedad: otra aplicacion que
/// eligiera el mismo patron por casualidad veria abortadas sus subidas
/// viejas. Es la misma precondicion que el sello de digest —el cubo es
/// de este servicio—, y con pid y nanos el empate real exige un reloj y
/// un numero de proceso identicos en dos maquinas contra el mismo cubo.
fn is_a_staging_key(key: &str) -> bool {
    let Some(stem) = key.strip_prefix(&format!("{STAGING_PREFIX}/")) else {
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

/// El sello de digest que este almacen estampa en cada objeto, o `None`
/// si el objeto no lleva sello —un preexistente de otra mano, un resto
/// que aun no se escribia con condicion.
fn the_stamp_of(
    head: &aws_sdk_s3::operation::head_object::HeadObjectOutput,
) -> Option<ContentDigest> {
    let name = head
        .metadata()
        .and_then(|items| items.get(DIGEST_METADATA))?
        .to_ascii_lowercase();
    if name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Some(ContentDigest::sha256(name))
    } else {
        None
    }
}

#[async_trait::async_trait]
impl BlobStore for S3BlobStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        // En un CAS la clave es el digest del contenido que se envia, asi
        // que publicar dos veces el mismo contenido no es un error y no
        // guarda dos veces. Lo que la escritura condicionada aporta sobre
        // la simple es la garantia de que el objeto que ya estaba **es**
        // el contenido: el PUT solo entra si la clave esta libre, y si el
        // servidor la rechaza, `settle_a_refused_write` comprueba el
        // preexistente en vez de dar por bueno un HEAD. Lo que no cuadra
        // se repara, nunca se deduplica.
        match self.conditional_put(&digest, bytes, true).await {
            Ok(()) => Ok(digest),
            Err(error) if is_precondition_failed(&error) => {
                // El `bytes.to_vec()` solo se paga en el camino del
                // rechazo, que en un CAS sano es la excepcion (un
                // preexistente sin sello o corrupto), no la dedupe.
                let owned = bytes.to_vec();
                let repair = {
                    let digest = digest.clone();
                    async move {
                        self.conditional_put(&digest, &owned, false)
                            .await
                            .map_err(|error| Self::unavailable("repair the content", error))
                    }
                };
                self.settle_a_refused_write(&digest, Some(bytes), repair)
                    .await?;
                Ok(digest)
            }
            Err(error) => Err(Self::unavailable("store the content", error)),
        }
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
            durable: false,
            covers_all: true,
        }))
    }

    /// Reabrir una subida multiple en curso a partir del handle del
    /// journal: `staging/<pid>-<nanos>-<counter>` y su `upload_id`.
    ///
    /// El almacén es la autoridad de lo que hay: se listan los trozos y
    /// se toma el **prefijo** —ordenado por numero— cuya suma es
    /// exactamente el offset que el journal declara. Un multipart con
    /// mas bytes que los declarados no es un error mientras exista ese
    /// prefijo: los trozos que la autoridad no registro son los de un
    /// proceso que murio entre el upload y el avance, se quedan fuera de
    /// la lista del complete y no se publican nunca; el ciclo de vida
    /// del cubo los recoge. Si ningun prefijo suma lo declarado —bytes
    /// solapados de dos escritoras—, el resume falla y la subida se
    /// rehace: publicar por delante de la autoridad esta prohibido.
    async fn resume(
        &self,
        handle: &str,
        written: u64,
    ) -> Result<Box<dyn hodei_hub::blob::BlobWriteSession>, BlobError> {
        let (key, upload_id) = handle.split_once('\u{0}').ok_or_else(|| {
            BlobError::Unavailable("the session handle is not a multipart".to_owned())
        })?;
        // Solo staging propio: una clave ajena bajo el prefijo no es una
        // subida que este almacen reanude, y tocarla no entra en el
        // contrato.
        if !is_a_staging_key(key) {
            return Err(BlobError::Unavailable(
                "the session handle is not a staging upload of this store".to_owned(),
            ));
        }
        let mut listed: Vec<(i32, u64, String)> = Vec::new();
        let mut marker: Option<String> = None;
        loop {
            let mut request = self
                .client
                .list_parts()
                .bucket(&self.config.bucket)
                .key(key)
                .upload_id(upload_id)
                .max_parts(1000);
            if let Some(marker) = &marker {
                request = request.part_number_marker(marker.clone());
            }
            let page = match request.send().await {
                Ok(page) => page,
                Err(error) if is_not_found(&error) => {
                    return Err(BlobError::Unavailable(
                        "the staging upload does not exist".to_owned(),
                    ));
                }
                Err(error) => {
                    return Err(Self::unavailable("list the parts of the session", error));
                }
            };
            for part in page.parts() {
                let number = part.part_number().unwrap_or(0);
                let size = part.size().unwrap_or(0) as u64;
                let e_tag = part.e_tag().unwrap_or_default().to_owned();
                listed.push((number, size, e_tag));
            }
            if page.is_truncated() != Some(true) {
                break;
            }
            marker = page.next_part_number_marker.map(|value| value.to_string());
            if marker.is_none() {
                break;
            }
        }
        listed.sort_by_key(|(number, _size, _tag)| *number);
        let mut parts: Vec<CompletedPart> = Vec::new();
        let mut highest = 0i32;
        let mut total = 0u64;
        for (number, size, e_tag) in &listed {
            if total + size > written {
                break;
            }
            total += size;
            highest = *number;
            parts.push(
                CompletedPart::builder()
                    .part_number(*number)
                    .e_tag(e_tag)
                    .build(),
            );
        }
        if total != written {
            return Err(BlobError::Unavailable(format!(
                "the staging upload does not hold a prefix of exactly {written} bytes and the journal declared {written}"
            )));
        }
        Ok(Box::new(S3BlobWriteSession {
            client: self.client.clone(),
            config: self.config.clone(),
            gate: Arc::clone(&self.gate),
            staging_key: key.to_owned(),
            upload_id: upload_id.to_owned(),
            buffer: Vec::new(),
            pending: VecDeque::new(),
            completed: parts,
            part_number: highest,
            hasher: Sha256::new(),
            written,
            open: true,
            // El journal es la autoridad del staging; el hasher local no
            // vio los trozos que ya estaban.
            durable: true,
            covers_all: written == 0,
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
    /// El journal del registro es quien decide el destino del multipart:
    /// el Drop del mango **no** lo aborta.
    durable: bool,
    /// Si el hasher local cubre el staging entero. Un multipart reabierto
    /// en frio no vuelve a ver los trozos que ya volaron: su digest local
    /// es parcial, y el commit tiene que comprobar los bytes contra el
    /// almacen, no contra el hasher.
    covers_all: bool,
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
    ///
    /// La copia va con la condicion de no-sobrescritura que el backend
    /// ofrezca (medida, HSP-139: `If-None-Match: *` sobre el destino en
    /// RustFS y en LocalStack; si un backend no la soporta, el backend
    /// degrada a la copia sin condicion —deduplicar por contenido sigue
    /// siendo cierto, dejar de pisar no—). El 412 no es un fallo: dos
    /// sesiones concurrentes del mismo contenido es exactamente el caso
    /// que la condicion resuelve, y el perdedor comprueba al ganador con
    /// el mismo criterio honesto que la via `put`.
    async fn publish(&self, target: &str, digest: &ContentDigest) -> Result<(), BlobError> {
        if self.completed.is_empty() {
            // Ni un byte: no hay nada que completar, y S3 no admite
            // subidas multiples vacias. El contenido vacio se publica
            // con un PUT normal, condicionado como el resto.
            let answer = self
                .client
                .put_object()
                .bucket(&self.config.bucket)
                .key(target)
                .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
                .if_none_match("*")
                .body(ByteStream::from_static(&[]))
                .send()
                .await;
            return match answer {
                Ok(_) => Ok(()),
                Err(error) if is_precondition_failed(&error) => {
                    // Alguien publico antes en esta clave, y aqui la
                    // asimetria con la via `put` no tiene defensa: el
                    // 412 de un cuerpo vacio se comprueba con el mismo
                    // criterio honesto que cualquier otro rechazo. Con
                    // las bytes sabidas —vacías— la comprobacion es la
                    // mas barata posible, y reparar es repetir el PUT
                    // sin condicion.
                    let store = S3BlobStore {
                        client: self.client.clone(),
                        config: self.config.clone(),
                        gate: self.gate.clone(),
                    };
                    let repair = async {
                        self.client
                            .put_object()
                            .bucket(&self.config.bucket)
                            .key(target)
                            .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
                            .body(ByteStream::from_static(&[]))
                            .send()
                            .await
                            .map(|_| ())
                            .map_err(|error| {
                                S3BlobStore::unavailable("repair the empty content", error)
                            })
                    };
                    store
                        .settle_a_refused_write(digest, Some(&[]), repair)
                        .await
                }
                Err(error) => Err(S3BlobStore::unavailable("publish the empty content", error)),
            };
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
        // La verificacion honesta de una sesion reabierta en frio —la
        // marca `covers_all` del mango: su hasher local no paso por los
        // trozos que otra réplica subio, asi que el commit no pudo
        // comparar un digest parcial contra lo declarado (daria un
        // `DigestMismatch` falso y abortaria una subida buena). Se lee
        // el objeto staging ya completado y se comprueba contra lo
        // declarado **antes** de copiarlo a la clave del blob. Lo que
        // no cuadra no se publica: se borra el staging y no queda
        // referencia.
        if !self.covers_all {
            let staged = self
                .client
                .get_object()
                .bucket(&self.config.bucket)
                .key(&self.staging_key)
                .send()
                .await
                .map_err(|error| S3BlobStore::unavailable("read the staged content", error))?;
            let bytes = staged
                .body
                .collect()
                .await
                .map_err(|error| S3BlobStore::unavailable("stream the staged content", error))?
                .into_bytes()
                .to_vec();
            let found = ContentDigest::of_bytes(&bytes);
            if found != *digest {
                // El contenido no es el declarado: el staging se borra
                // y no se copia nada. Un blob con un digest que no es el
                // suyo no es un blob.
                let _ = self
                    .client
                    .delete_object()
                    .bucket(&self.config.bucket)
                    .key(&self.staging_key)
                    .send()
                    .await;
                return Err(BlobError::DigestMismatch {
                    expected: digest.clone(),
                    found,
                });
            }
        }
        let copied = if self.written > MAX_SIMPLE_COPY {
            self.copy_as_multipart(target, digest, true).await
        } else {
            // La copia es server-side: los bytes no vuelven a pasar por
            // el proceso, y solo entra si el destino esta libre.
            match self
                .client
                .copy_object()
                .bucket(&self.config.bucket)
                .key(target)
                .copy_source(format!("{}/{}", self.config.bucket, self.staging_key))
                .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
                .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace)
                .if_none_match("*")
                .send()
                .await
            {
                Ok(_) => Ok(true),
                Err(error) if is_precondition_failed(&error) => Ok(false),
                Err(error) => Err(S3BlobStore::unavailable("publish the blob", error)),
            }
        };
        let copied = match copied {
            // El camino normal: la copia entro, o el fallo ya esta dicho.
            Ok(true) => Ok(()),
            // Alguien publico primero. Que sea ESTE contenido lo
            // comprueba la misma decision honesta que la via `put`:
            // sello, o lectura; lo que no cuadre, se repara. Aqui no
            // tenemos los bytes en el proceso —estan en el almacén,
            // bajo la clave de staging ya completada—, asi que la
            // reparacion es volver a copiar sin condicion. La vista del
            // store es barata (el cliente es un Arc) y comparte la
            // barrera: quien llama sostiene el gate desde el commit.
            Ok(false) => {
                let store = S3BlobStore {
                    client: self.client.clone(),
                    config: self.config.clone(),
                    gate: self.gate.clone(),
                };
                let repair = self.copy_unconditioned(target, digest);
                store.settle_a_refused_write(digest, None, repair).await
            }
            Err(error) => Err(error),
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

    /// La copia server-side sin la condicion: el camino de la reparacion
    /// de una publicacion, cuando el preexistente no es el contenido.
    async fn copy_unconditioned(
        &self,
        target: &str,
        digest: &ContentDigest,
    ) -> Result<(), BlobError> {
        if self.written > MAX_SIMPLE_COPY {
            return self
                .copy_as_multipart(target, digest, false)
                .await
                .map(|_| ());
        }
        self.client
            .copy_object()
            .bucket(&self.config.bucket)
            .key(target)
            .copy_source(format!("{}/{}", self.config.bucket, self.staging_key))
            .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
            .metadata_directive(aws_sdk_s3::types::MetadataDirective::Replace)
            .send()
            .await
            .map(|_| ())
            .map_err(|error| S3BlobStore::unavailable("repair the published blob", error))
    }

    /// Copiar un objeto de mas de cinco gigabytes: `CopyObject` no llega
    /// alli, asi que la copia se hace trozo a trozo, con ranges sobre la
    /// clave de staging. Es lo mismo que hace el desplegador maduro con
    /// s3-sr-copy y aqui no puede ser un paso externo: el commit es o todo
    /// o nada.
    ///
    /// La condicion de no-sobrescritura va sobre el `Complete`: el
    /// destino no existe hasta que la copia multiple cierra, que es el
    /// punto atomico que S3 promete. Devuelve si la copia escribio:
    /// `false` es el 412, el "ya estaba" que el llamante resuelve.
    async fn copy_as_multipart(
        &self,
        target: &str,
        digest: &ContentDigest,
        only_if_absent: bool,
    ) -> Result<bool, BlobError> {
        let created = self
            .client
            .create_multipart_upload()
            .bucket(&self.config.bucket)
            .key(target)
            .set_metadata(Some(S3BlobStore::the_stamp_map(digest)))
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
        let mut request = self
            .client
            .complete_multipart_upload()
            .bucket(&self.config.bucket)
            .key(target)
            .upload_id(&upload_id)
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            );
        if only_if_absent {
            request = request.if_none_match("*");
        }
        match request.send().await {
            Ok(_) => Ok(true),
            Err(error) if only_if_absent && is_precondition_failed(&error) => {
                // El destino ya estaba: la copia no hizo falta. Pero el
                // multipart que abrimos **sobre la clave de destino**
                // sigue vivo en el servidor, y su clave no es de
                // staging —el segado no la mira y `list_blobs` no la
                // ve—. Se aborta aqui, antes de ceder el veredicto al
                // llamante; dejarla seria cambiar una carrera
                // inocua por una fuga permanente de trozos pagados.
                let _ = self
                    .client
                    .abort_multipart_upload()
                    .bucket(&self.config.bucket)
                    .key(target)
                    .upload_id(&upload_id)
                    .send()
                    .await;
                Ok(false)
            }
            Err(error) => Err(S3BlobStore::unavailable("finish the copy", error)),
        }
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

        if self.written != expectation.length {
            self.discard().await;
            return Err(BlobError::LengthMismatch {
                expected: expectation.length,
                found: self.written,
            });
        }

        // La comprobacion honesta del digest. Cuando el mango paso por
        // todos los bytes, el hasher local es la verdad y se compara
        // aqui, barato. Cuando la sesion se reablo en frio —el journal
        // la declaro y otra réplica escribio los primeros trozos—, el
        // hasher local es **parcial**: compararlo contra lo declarado
        // daria un `DigestMismatch` falso y abortaria una subida buena.
        // En ese caso la comprobacion la hace `publish` contra el
        // staging ya completado: la misma verificacion honesta, una
        // sola vez, al publicar.
        let declared = expectation.digest;
        if self.covers_all {
            let found = ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()));
            if found != declared {
                self.discard().await;
                return Err(BlobError::DigestMismatch {
                    expected: declared,
                    found,
                });
            }
        }

        // Desde aqui hasta publicar, el recolector no puede borrar nada.
        let barrier = self.gate.lock().await;
        let target = blob_key(&declared);
        let outcome = self.publish(&target, &declared).await;
        drop(barrier);
        self.open = false;
        outcome?;
        Ok(declared)
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

    /// Lo que el almacén tiene: los bytes en vuelo no son durable hasta
    /// que el servidor los confirma, y lo que queda en el buffer no salio
    /// del proceso. Es el offset que el journal puede declarar.
    fn the_durable_offset(&self) -> u64 {
        self.written - self.buffer.len() as u64
    }

    fn the_digest(&self) -> ContentDigest {
        ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()))
    }

    fn the_staging_path(&self) -> Option<&std::path::Path> {
        None
    }

    /// El handle del journal: la clave de staging y el `upload_id`, unidos
    /// por un cero que ninguno de los dos puede llevar.
    fn the_staging_handle(&self) -> Option<String> {
        Some(format!("{}\u{0}{}", self.staging_key, self.upload_id))
    }

    /// Un multipart reabierto en frio no volvio a ver sus trozos: su
    /// hasher local es parcial, y el commit tiene que comprobar los bytes
    /// contra el almacén.
    fn the_digest_covers_all(&self) -> bool {
        self.covers_all
    }

    /// Suelta el mango **solo si no queda nada en el buffer**.
    ///
    /// El multipart no tiene un staging intermedio al que puedan volver
    /// los bytes del buffer: soltarse con bytes dentro los perderia, y
    /// la autoridad del journal no los declaro. Con el buffer vacio y
    /// las partes en vuelo ya recogidas por `flush_durable`, soltar es
    /// cerrar sin abortar: el multipart sigue vivo y el proximo `PATCH`
    /// lo retoma por su handle, aqui o en otra réplica.
    fn detach(&mut self) -> Result<(), BlobError> {
        if !self.buffer.is_empty() {
            return Err(BlobError::Unavailable(
                "a multipart session with bytes in the buffer does not release its writer"
                    .to_owned(),
            ));
        }
        self.open = false;
        for handle in self.pending.drain(..) {
            handle.abort();
        }
        Ok(())
    }

    /// El journal toma la propiedad del multipart: el Drop ya no lo aborta.
    ///
    /// Un mango soltado o un proceso que muere NO aborta la subida de
    /// otra réplica; su destino lo decide la autoridad: commit, abort
    /// explicito o barrido. El resume comprueba contra `ListParts` que
    /// el almacén tiene exactamente los bytes que la autoridad declara
    /// y rehacer la subida es el unico camino si un trozo duplicado puso
    /// el staging por delante del journal. No se publica nunca por
    /// delante de la autoridad.
    fn mark_durable(&mut self) {
        self.durable = true;
    }

    /// Recoge las partes en vuelo y declara el offset durable.
    ///
    /// Lo confirmado por el servidor es durable; lo que queda en el
    /// buffer no sale del proceso, y el cliente lo reenvia contra el
    /// `Range` que la autoridad declara.
    async fn flush_durable(&mut self) -> Result<u64, BlobError> {
        while let Some(handle) = self.pending.pop_front() {
            self.collect_part(tokio::time::timeout(UPLOAD_TIMEOUT, handle).await)?;
        }
        Ok(self.written - self.buffer.len() as u64)
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
        // Con journal, el destino del multipart lo decide la autoridad:
        // un mango local que muere (cache desalojado, réplica que sigue)
        // NO aborta la subida de otra réplica.
        if self.durable {
            return;
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
