//! The HSP-139 conformance, written once and run against each
//! server: the same battery of concurrency, real conditional write, staging
//! reaping and capabilities report applies to the emulator
//! (LocalStack) and to the real S3 server (RustFS). The verdicts of the
//! report carry the version note of the bench that produced them: a
//! report without "against what" is not a report.

use std::time::Duration;

use aws_sdk_s3::Client;
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::ChecksumAlgorithm;
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobError, BlobStore, CommitExpectation};
use hodei_hub_s3::{S3BlobConfig, S3BlobStore};

/// The body of the boundary tests: deterministic, so that its
/// digest is the same on any bench.
pub fn the_bulk(seed: u8, len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
        .collect()
}

/// A store with 5 MiB parts and two in flight: the S3 minimum and a
/// small budget, so that the boundaries are crossed with bodies
/// that fit in memory without deceiving the server.
pub async fn a_store_with_parts(client: &Client, bucket: &str, part_size: usize) -> S3BlobStore {
    S3BlobStore::new(
        client.clone(),
        S3BlobConfig {
            bucket: bucket.to_string(),
            part_size,
            max_inflight_parts: 2,
        },
    )
    .expect("el almacen abre")
}

/// AC1: N concurrent publishers of the same content converge to one
/// intact object and a single reference per digest, over the real
/// multipart: two uploads at once against the same destination key.
///
/// Each publisher brings its **own store**, not a shared one: the memory
/// semaphore lives in the store, and if we all used the same one
/// the real concurrency would collapse into a queue ordered by that
/// semaphore. Two stores over the same client are two processes that
/// publish at once without any coordination between them --which is
/// exactly what is to be measured.
pub async fn ac1_concurrent_publishers(client: &Client, bucket: &str) {
    let bytes = the_bulk(7, 13 * 1024 * 1024);
    let expectation = CommitExpectation::of(&bytes);

    let mut tasks = Vec::new();
    for _ in 0..6 {
        let client = client.clone();
        let bucket = bucket.to_string();
        let bytes = bytes.clone();
        let expectation = expectation.clone();
        tasks.push(tokio::spawn(async move {
            let store = a_store_with_parts(&client, &bucket, 5 * 1024 * 1024).await;
            let mut session = store.begin().await.expect("la sesion abre");
            for chunk in bytes.chunks(4 * 1024 * 1024) {
                session.append(chunk).await.expect("el trozo entra");
            }
            session
                .commit(expectation)
                .await
                .expect("la publicacion converge");
        }));
    }
    for task in tasks {
        task.await.expect("el publicador no paniquea");
    }

    let digest = ContentDigest::of_bytes(&bytes);
    let store = a_store_with_parts(client, bucket, 5 * 1024 * 1024).await;
    store.verify(&digest).await.expect("el objeto esta integro");
    assert_eq!(
        store.length(&digest).await.expect("la longitud"),
        bytes.len() as u64,
        "AC1: la concurrencia no puede cambiar los bytes"
    );
    let listed = store
        .reclaimer()
        .expect("el reclaimer")
        .list_blobs()
        .await
        .expect("el listado sale");
    assert_eq!(
        listed.iter().filter(|blob| blob.digest == digest).count(),
        1,
        "AC1: una clave, no dos referencias del mismo digest"
    );
}

/// AC1 (retry half): publishing the same content three times
/// in a row leaves ONE object with ITS bytes, whether it already existed or not.
///
/// What this test does **not** do, stated plainly: it does not
/// inject the loss of `complete` in the network. The lost response
/// of the statement is modeled by its observable effect --a caller that
/// retries without knowing--, and what is tested is the idempotency of the
/// retry over the destination key. The real race is measured by the
/// concurrency test above; here what is pinned is that the
/// third attempt on an already sealed object does not write a second
/// reference nor touch the first. A server failure injected by
/// proxy is recorded debt, not a guarantee this test promises.
pub async fn ac1_lost_ack_retry(client: &Client, bucket: &str) {
    let store = a_store_with_parts(client, bucket, 5 * 1024 * 1024).await;
    let bytes = the_bulk(31, 6 * 1024 * 1024);
    let digest = ContentDigest::of_bytes(&bytes);
    let expectation = CommitExpectation::of(&bytes);

    for attempt in 0..3 {
        let mut session = store.begin().await.expect("la sesion abre");
        for chunk in bytes.chunks(2 * 1024 * 1024) {
            session.append(chunk).await.expect("el trozo entra");
        }
        session
            .commit(expectation.clone())
            .await
            .unwrap_or_else(|error| panic!("el reintento {attempt} no debio fallar: {error}"));
    }
    store
        .verify(&digest)
        .await
        .expect("integro tras los reintentos");
    let listed = store
        .reclaimer()
        .expect("el reclaimer")
        .list_blobs()
        .await
        .expect("el listado sale");
    assert_eq!(
        listed.iter().filter(|blob| blob.digest == digest).count(),
        1,
        "AC1: tres intentos, una referencia"
    );
}

/// AC2: the honesty of the conditional dedupe. A foreign body without a seal
/// is planted at the key of a digest; `verify` calls it
/// corrupt, `put` repairs it, and the second publication of the good
/// content really deduplicates over the already sealed object.
pub async fn ac2_corrupt_is_never_dedupe(client: &Client, bucket: &str) {
    let store = S3BlobStore::new(client.clone(), S3BlobConfig::new(bucket)).expect("abre");
    let bytes = the_bulk(11, 4096);
    let digest = ContentDigest::of_bytes(&bytes);
    let key = store.the_key_of(&digest);

    client
        .put_object()
        .bucket(bucket)
        .key(&key)
        .body(the_bulk(12, 4096).into())
        .send()
        .await
        .expect("el cuerpo ajeno se planta en la clave");

    let error = store
        .verify(&digest)
        .await
        .expect_err("AC2: un preexistente que no es el digest no es dedupe");
    assert!(
        matches!(error, BlobError::Corrupted { .. }),
        "AC2: se dice integridad, no disponibilidad ni éxito: {error}"
    );

    let published = store.put(&bytes).await.expect("AC2: publicar repara");
    assert_eq!(published, digest);
    store
        .verify(&digest)
        .await
        .expect("integro tras la reparacion");
    assert_eq!(store.get(&digest).await.expect("se lee"), bytes);

    let again = store
        .put(&bytes)
        .await
        .expect("AC2: repetir no es un error");
    assert_eq!(again, digest);
    store.verify(&digest).await.expect("AC2: sigue integro");
    assert_eq!(
        store.length(&digest).await.expect("la longitud"),
        bytes.len() as u64,
        "AC2: la segunda publicacion no escribio otra cosa"
    );

    // The empty branch of the publish, checked the same as the rest. The
    // empty content does not upload multipart, so its publication is a
    // conditional PUT and its 412 has to go through the same honest
    // criterion: if at the key of the empty digest there is something else, that is not
    // the empty one and it is repaired --it is not considered published.
    let empty = ContentDigest::of_bytes(b"");
    let empty_key = store.the_key_of(&empty);
    client
        .put_object()
        .bucket(bucket)
        .key(&empty_key)
        .body(the_bulk(99, 2048).into())
        .send()
        .await
        .expect("el cuerpo ajeno se planta en la clave del vacio");
    let mut session = store.begin().await.expect("la sesion vacia abre");
    session
        .commit(CommitExpectation::of(b""))
        .await
        .expect("AC2: publicar el vacio sobre el preexistente ajeno repara");
    assert_eq!(
        store.get(&empty).await.expect("el vacio se lee"),
        Vec::<u8>::new(),
        "AC2: la clave del vacio contiene vacio, no lo que se planto"
    );
}

/// AC2 (session half): the conditional publish of the session on
/// a key that already has a good object deduplicates without touching it.
pub async fn ac2_session_dedupes(client: &Client, bucket: &str) {
    let store = a_store_with_parts(client, bucket, 5 * 1024 * 1024).await;
    let bytes = the_bulk(13, 12 * 1024 * 1024);
    let digest = ContentDigest::of_bytes(&bytes);

    store.put(&bytes).await.expect("la via simple publica");
    let mut session = store.begin().await.expect("la sesion abre");
    for chunk in bytes.chunks(4 * 1024 * 1024) {
        session.append(chunk).await.expect("el trozo entra");
    }
    session
        .commit(CommitExpectation::of(&bytes))
        .await
        .expect("AC2: el publish sobre el existente deduplica");
    store
        .verify(&digest)
        .await
        .expect("el objeto sigue integro");
    let listed = store
        .reclaimer()
        .expect("el reclaimer")
        .list_blobs()
        .await
        .expect("el listado sale");
    assert_eq!(
        listed.iter().filter(|blob| blob.digest == digest).count(),
        1,
        "AC2: una publicacion simple y una de sesion, una sola referencia"
    );
}

/// AC3: multipart at the boundary and clean abort. 11 MiB = 5+5+1 with
/// 5 MiB parts (the last below the minimum, the boundary that S3
/// allows) travels and comes back byte for byte; an aborted session leaves neither
/// object nor upload; the memory budget is the declared one.
pub async fn ac3_multipart_and_abort(client: &Client, bucket: &str) {
    let part = 5 * 1024 * 1024;
    let store = a_store_with_parts(client, bucket, part).await;

    let bytes = the_bulk(21, 11 * 1024 * 1024);
    let digest = ContentDigest::of_bytes(&bytes);
    let mut session = store.begin().await.expect("la sesion abre");
    for chunk in bytes.chunks(part) {
        session.append(chunk).await.expect("el trozo entra");
    }
    session
        .commit(CommitExpectation::of(&bytes))
        .await
        .expect("el multipart de frontera commitea");
    assert_eq!(
        store.get(&digest).await.expect("multipart leido"),
        bytes,
        "AC3: los bytes de tres trozos son los bytes de uno"
    );

    let abandoned = the_bulk(22, 12 * 1024 * 1024);
    let mut session = store.begin().await.expect("otra sesion abre");
    session
        .append(&abandoned[..part])
        .await
        .expect("un trozo entra");
    session.abort().await.expect("la sesion se aborta");
    let leftovers = client
        .list_multipart_uploads()
        .bucket(bucket)
        .prefix("staging/")
        .send()
        .await
        .expect("los uploads se listan");
    assert!(
        leftovers.uploads().is_empty(),
        "AC3: el abort no deja uploads multiparte huerfanos"
    );
    let objects = client
        .list_objects_v2()
        .bucket(bucket)
        .prefix("staging/")
        .send()
        .await
        .expect("el cubo se lista");
    assert!(
        objects.contents().is_empty(),
        "AC3: el staging abortado no deja objetos visibles"
    );

    let config = S3BlobConfig {
        bucket: bucket.to_string(),
        part_size: part,
        max_inflight_parts: 2,
    };
    assert_eq!(config.the_memory_budget(), 2 * part);
    assert!(
        config.the_memory_budget() < 11 * 1024 * 1024,
        "AC3: el presupuesto es acotado, no proporcional al cuerpo"
    );
}

/// AC3 (GC half): the reaping of orphaned staging. An upload with its
/// own key abandoned without aborting is aborted; an old staging object
/// is deleted; a **foreign** key under the prefix is not touched (the
/// GC discriminates); and a young upload is declared alive and respected.
pub async fn ac3_staging_reaper(client: &Client, bucket: &str) {
    let store = a_store_with_parts(client, bucket, 5 * 1024 * 1024).await;

    let orphan_key = format!("staging/{}-{:x}-{:x}", std::process::id(), 1u64 << 40, 0u8);
    let _orphan_id = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(&orphan_key)
        .send()
        .await
        .expect("la subida huerfana abre")
        .upload_id
        .expect("el id");
    let foreign_key = "staging/not-ours";
    let foreign_id = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(foreign_key)
        .send()
        .await
        .expect("la subida ajena abre")
        .upload_id
        .expect("el id");
    client
        .put_object()
        .bucket(bucket)
        .key(format!(
            "staging/{}-{:x}-{:x}",
            std::process::id(),
            2u64 << 40,
            1u8
        ))
        .body(ByteStream::from_static(b"resto"))
        .send()
        .await
        .expect("el objeto de staging entra");

    // With max_age zero everything found is of age; in a bucket
    // exclusive to this test there is no live session to protect.
    let cleanup = store
        .clean_staging(Duration::ZERO)
        .await
        .expect("el segado corre");
    assert!(
        cleanup.failed.is_empty(),
        "AC3: el segado fallo en algo: {:?}",
        cleanup.failed
    );
    assert_eq!(cleanup.alive, 0, "AC3: aqui no hay subidas vivas");
    assert!(
        cleanup.removed >= 2,
        "AC3: abort + objeto viejo, al menos dos: {cleanup:?}"
    );

    let surviving: Vec<String> = client
        .list_multipart_uploads()
        .bucket(bucket)
        .send()
        .await
        .expect("los uploads se listan")
        .uploads()
        .iter()
        .filter_map(|upload| upload.key().map(String::from))
        .collect();
    assert!(
        !surviving.contains(&orphan_key),
        "AC3: la subida propia huerfana fue abortada"
    );
    assert!(
        surviving.contains(&foreign_key.to_string()),
        "AC3: la subida ajena bajo el prefijo NO se toca —el GC distingue: {surviving:?}"
    );
    let _ = client
        .abort_multipart_upload()
        .bucket(bucket)
        .key(foreign_key)
        .upload_id(&foreign_id)
        .send()
        .await;

    let live_key = format!("staging/{}-{:x}-{:x}", std::process::id(), 3u64 << 40, 2u8);
    let live_id = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(&live_key)
        .send()
        .await
        .expect("la subida joven abre")
        .upload_id
        .expect("el id");
    let cleanup = store
        .clean_staging(Duration::from_secs(86_400))
        .await
        .expect("el segado corre");
    assert_eq!(cleanup.alive, 1, "AC3: la subida joven esta escribiendose");
    assert_eq!(cleanup.removed, 0, "AC3: y no se toca");
    let _ = client
        .abort_multipart_upload()
        .bucket(bucket)
        .key(&live_key)
        .upload_id(&live_id)
        .send()
        .await;
}

/// AC4: the capabilities report, measured against this bench and this
/// version. Each line is a real call; the assertions pin what
/// the product needs from it. Returns the report so that the caller
/// prints it with the version note of the server.
pub async fn ac4_capability_report(client: &Client, bucket: &str) -> Vec<String> {
    let mut report = Vec::new();
    let key = "blobs/informe/capacidad";
    let body = b"capacidad".to_vec();

    // Conditional write (If-None-Match: *): the AC2 mechanism,
    // measured as a backend capability, not as an SDK promise.
    let first = client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(ByteStream::from(body.clone()))
        .if_none_match("*")
        .send()
        .await;
    let conditional = first.is_ok();
    let second = client
        .put_object()
        .bucket(bucket)
        .key(key)
        .body(ByteStream::from(body.clone()))
        .if_none_match("*")
        .send()
        .await;
    let refused = match &second {
        Err(error) => ProvideErrorMetadata::code(error) == Some("PreconditionFailed"),
        Ok(_) => false,
    };
    report.push(format!(
        "if-none-match-estrella: soportado={conditional} rechaza-duplicado={refused}"
    ));

    // Object checksum: if the server computes it and serves it, the
    // client could rely on it; if not, verification is by recomputed
    // digest --which is what the product does.
    let with_checksum = client
        .put_object()
        .bucket(bucket)
        .key("blobs/informe/checksum")
        .body(ByteStream::from(body.clone()))
        .checksum_algorithm(ChecksumAlgorithm::Sha256)
        .send()
        .await;
    let head = client
        .head_object()
        .bucket(bucket)
        .key("blobs/informe/checksum")
        .send()
        .await;
    let checksum_served = head
        .as_ref()
        .ok()
        .and_then(|h| h.checksum_sha256().map(String::from));
    report.push(format!(
        "checksum-sha256: put_ok={} head_devuelve_checksum={checksum_served:?}",
        with_checksum.is_ok()
    ));

    // The ETag of a multipart is NOT the sha256 of the body: the lesson
    // of the statement ("do not assume ETag=SHA256"), measured against the bench.
    let multipart_body = the_bulk(41, 5 * 1024 * 1024);
    let multipart_etag = {
        let created = client
            .create_multipart_upload()
            .bucket(bucket)
            .key("blobs/informe/multipart")
            .send()
            .await
            .expect("el multipart abre");
        let upload_id = created.upload_id.expect("el id");
        let part = client
            .upload_part()
            .bucket(bucket)
            .key("blobs/informe/multipart")
            .upload_id(&upload_id)
            .part_number(1)
            .body(ByteStream::from(multipart_body.clone()))
            .send()
            .await
            .expect("el trozo entra");
        client
            .complete_multipart_upload()
            .bucket(bucket)
            .key("blobs/informe/multipart")
            .upload_id(&upload_id)
            .multipart_upload(
                aws_sdk_s3::types::CompletedMultipartUpload::builder()
                    .parts(
                        aws_sdk_s3::types::CompletedPart::builder()
                            .part_number(1)
                            .set_e_tag(part.e_tag().map(String::from))
                            .build(),
                    )
                    .build(),
            )
            .send()
            .await
            .expect("el multipart cierra")
            .e_tag
            .unwrap_or_default()
    };
    let sha_of_body = ContentDigest::of_bytes(&multipart_body).value;
    report.push(format!(
        "etag-multipart={multipart_etag} sha256-del-cuerpo={sha_of_body} coinciden={}",
        multipart_etag.trim_matches('"') == sha_of_body
    ));

    // Versioning: if the bucket supports it, a delete leaves a marker; if
    // not, the GC cannot rely on versions and its word is the
    // key. It is measured, not assumed.
    let versioning = client
        .get_bucket_versioning()
        .bucket(bucket)
        .send()
        .await
        .map(|answer| format!("{:?}", answer.status()))
        .unwrap_or_else(|error| format!("no disponible: {error}"));
    report.push(format!("versionado: {versioning}"));

    // Range: GET with Range, the path of `read_range` without fetching the whole.
    let ranged = client
        .get_object()
        .bucket(bucket)
        .key(key)
        .range("bytes=0-3")
        .send()
        .await;
    let range_bytes = match ranged {
        Ok(got) => got
            .body
            .collect()
            .await
            .map(|aggregate| aggregate.into_bytes().len())
            .ok(),
        Err(_) => None,
    };
    report.push(format!("rango: servido={range_bytes:?} esperado=4"));

    // The server-side copy of the publish over a boundary body; the
    // simple 5 GiB limit is declared by the code and is not measured here:
    // it exceeds this machine's bench with room to spare.
    let store = a_store_with_parts(client, bucket, 5 * 1024 * 1024).await;
    let boundary = the_bulk(43, 11 * 1024 * 1024);
    let mut session = store.begin().await.expect("la sesion abre");
    for chunk in boundary.chunks(5 * 1024 * 1024) {
        session.append(chunk).await.expect("el trozo entra");
    }
    session
        .commit(CommitExpectation::of(&boundary))
        .await
        .expect("la copia de publicacion frontera sale");
    report.push(
        "copia-de-publicacion: ok sobre 11 MiB con trozos de 5 MiB; limite simple declarado por el codigo = 5 GiB (no medido: excede la mesa)"
            .to_string(),
    );

    // What the product needs becomes an assertion, not just a report.
    assert!(
        conditional && refused,
        "AC4: el backend debe soportar If-None-Match medido: {report:?}"
    );
    assert_eq!(
        range_bytes,
        Some(4),
        "AC4: el rango de 4 bytes debe servir 4 bytes: {report:?}"
    );
    assert!(
        multipart_etag.trim_matches('"') != sha_of_body,
        "AC4: si algún día el ETag multipart fuera el sha256, cambiar el contrato (verificado por digest) sería una decisión, no un accidente: {report:?}"
    );
    report
}
