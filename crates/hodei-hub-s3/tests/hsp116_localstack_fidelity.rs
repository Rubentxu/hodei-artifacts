//! La fidelidad del servidor S3 de pruebas para las operaciones que el
//! puerto BlobStore necesita.
//!
//! No es un test del backend —lo es `hsp116_s3_contract`—: es la
//! comprobación de que el servidor que pisa el arnes sirve exactamente
//! las llamadas sobre las que se apoya `S3BlobStore`. Si alguna de estas
//! garantías falta, el diseño del backend cambia antes de fiar nada de
//! él.

mod common;

use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use hodei_hub_s3::S3BlobConfig;

async fn a_bucket(name: &str) -> (aws_sdk_s3::Client, String) {
    let bucket = common::localstack::a_unique_bucket(name);
    let client = common::localstack::a_client().await;
    common::localstack::a_bucket(&client, &bucket).await;
    (client, bucket)
}

/// put/get/head/delete/list: la superficie minima del CAS.
#[tokio::test]
async fn the_server_serves_the_basic_object_operations() {
    let (client, bucket) = a_bucket("basic").await;

    let body = b"the bytes of the artifact";
    let sent = ByteStream::from_static(body);
    client
        .put_object()
        .bucket(&bucket)
        .key("blobs/ab/cd/thekey")
        .body(sent)
        .send()
        .await
        .expect("el objeto se guarda");

    let head = client
        .head_object()
        .bucket(&bucket)
        .key("blobs/ab/cd/thekey")
        .send()
        .await
        .expect("el head dice que esta");
    assert_eq!(head.content_length.unwrap(), body.len() as i64);
    assert!(
        head.last_modified.is_some(),
        "head da fecha: el GC vive de ella"
    );

    let got = client
        .get_object()
        .bucket(&bucket)
        .key("blobs/ab/cd/thekey")
        .send()
        .await
        .expect("el objeto se lee");
    assert_eq!(
        got.body.collect().await.unwrap().to_vec(),
        b"the bytes of the artifact"
    );

    let listed = client
        .list_objects_v2()
        .bucket(&bucket)
        .prefix("blobs/")
        .send()
        .await
        .expect("el listado sale");
    let keys: Vec<_> = listed
        .contents()
        .iter()
        .flat_map(|object| object.key())
        .map(|key| key.to_string())
        .collect();
    assert_eq!(keys, vec!["blobs/ab/cd/thekey".to_string()]);

    client
        .delete_object()
        .bucket(&bucket)
        .key("blobs/ab/cd/thekey")
        .send()
        .await
        .expect("el objeto se borra");
    let error = client
        .head_object()
        .bucket(&bucket)
        .key("blobs/ab/cd/thekey")
        .send()
        .await
        .expect_err("borrado, no esta");
    assert!(
        matches!(error.code(), Some("NotFound") | Some("NoSuchKey")),
        "el missing es un 404 de servicio, no un fallo de red"
    );
}

/// Range reads: la via del `read_range` del puerto.
#[tokio::test]
async fn the_server_serves_byte_ranges() {
    let (client, bucket) = a_bucket("range").await;

    let bytes: Vec<u8> = (0u8..=255).collect();
    client
        .put_object()
        .bucket(&bucket)
        .key("blobs/range")
        .body(ByteStream::from(bytes.clone()))
        .send()
        .await
        .expect("el contenido esta");

    let got = client
        .get_object()
        .bucket(&bucket)
        .key("blobs/range")
        .range("bytes=10-14")
        .send()
        .await
        .expect("el tramo se sirve");
    assert_eq!(got.body.collect().await.unwrap().to_vec(), bytes[10..15]);

    // Un tramo que asoma del final: S3 devuelve lo que hay, truncado.
    let short = client
        .get_object()
        .bucket(&bucket)
        .key("blobs/range")
        .range("bytes=250-259")
        .send()
        .await
        .expect("el tramo corto se sirve truncado");
    assert_eq!(
        short.body.collect().await.unwrap().to_vec(),
        bytes[250..],
        "S3 recorta el tramo, no lo inventa"
    );

    // Un offset pasado el final: InvalidRange.
    let error = client
        .get_object()
        .bucket(&bucket)
        .key("blobs/range")
        .range("bytes=1000-1010")
        .send()
        .await
        .expect_err("el offset no existe");
    assert!(
        error.code() == Some("InvalidRange"),
        "el offset pasado es un InvalidRange: {error}"
    );
}

/// Multipart completo: crear, subir trozos con su Content-MD5, listarlos
/// y completar: el camino entero de una sesion de escritura.
#[tokio::test]
async fn the_server_serves_multipart_uploads() {
    let (client, bucket) = a_bucket("multipart").await;

    let created = client
        .create_multipart_upload()
        .bucket(&bucket)
        .key("blobs/multi")
        .send()
        .await
        .expect("la subida multiple se crea");
    let upload_id = created.upload_id().expect("un id de subida").to_string();

    // S3 exige que todos los trozos menos el ultimo tengan >= 5 MiB,
    // incluido el minimo que el propio backend configura.
    let part_one = vec![7u8; S3BlobConfig::new("x").part_size];
    let part_two = b"the tail".to_vec();
    let mut parts = Vec::new();
    for (number, body) in [(1, &part_one), (2, &part_two)] {
        let md5 = {
            use base64::Engine;
            use md5::Digest;
            base64::engine::general_purpose::STANDARD.encode(md5::Md5::digest(body).as_slice())
        };
        let sent = client
            .upload_part()
            .bucket(&bucket)
            .key("blobs/multi")
            .upload_id(&upload_id)
            .part_number(number)
            .content_md5(md5)
            .body(ByteStream::from(body.clone()))
            .send()
            .await
            .expect("el trozo con su MD5 se sube");
        parts.push(
            CompletedPart::builder()
                .part_number(number)
                .set_e_tag(sent.e_tag().map(|etag| etag.to_string()))
                .build(),
        );
    }

    let listed = client
        .list_parts()
        .bucket(&bucket)
        .key("blobs/multi")
        .upload_id(&upload_id)
        .send()
        .await
        .expect("los trozos se listan");
    assert_eq!(listed.parts().len(), 2, "el servidor cuenta los dos trozos");

    client
        .complete_multipart_upload()
        .bucket(&bucket)
        .key("blobs/multi")
        .upload_id(&upload_id)
        .multipart_upload(
            CompletedMultipartUpload::builder()
                .set_parts(Some(parts))
                .build(),
        )
        .send()
        .await
        .expect("la subida se completa");

    let head = client
        .head_object()
        .bucket(&bucket)
        .key("blobs/multi")
        .send()
        .await
        .expect("el objeto completado esta");
    assert_eq!(
        head.content_length.unwrap(),
        (part_one.len() + part_two.len()) as i64
    );
}

/// Un MD5 mentiroso en un trozo: la condicion de integridad server-side
/// existe y se llama BadDigest.
#[tokio::test]
async fn the_server_checks_part_content_md5() {
    let (client, bucket) = a_bucket("md5").await;

    let created = client
        .create_multipart_upload()
        .bucket(&bucket)
        .key("blobs/md5")
        .send()
        .await
        .expect("la subida se crea");
    let upload_id = created.upload_id().expect("un id de subida").to_string();

    let error = client
        .upload_part()
        .bucket(&bucket)
        .key("blobs/md5")
        .upload_id(&upload_id)
        .part_number(1)
        .content_md5("AAAAAAAAAAAAAAAAAAAAAA==")
        .body(ByteStream::from(b"the bytes to hash".to_vec()))
        .send()
        .await
        .expect_err("un trozo con MD5 mentiroso se rechaza");
    assert!(
        error.code() == Some("BadDigest"),
        "el MD5 mentiroso es un BadDigest: {error}"
    );
}

/// Abort de una subida multiple: el staging caducado tambien hay que
/// tirarlo, y abortar no publica.
#[tokio::test]
async fn the_server_aborts_multipart_uploads() {
    let (client, bucket) = a_bucket("abort").await;

    let created = client
        .create_multipart_upload()
        .bucket(&bucket)
        .key("blobs/abandoned")
        .send()
        .await
        .expect("la subida se crea");
    let upload_id = created.upload_id().expect("un id de subida").to_string();

    client
        .abort_multipart_upload()
        .bucket(&bucket)
        .key("blobs/abandoned")
        .upload_id(&upload_id)
        .send()
        .await
        .expect("la subida se abandona");

    client
        .head_object()
        .bucket(&bucket)
        .key("blobs/abandoned")
        .send()
        .await
        .expect_err("abortar no publica");
}

/// La publicacion del commit es una copia server-side dentro del mismo
/// cubo: si eso no funciona, el diseno del staging se cae.
#[tokio::test]
async fn the_server_copies_server_side() {
    let (client, bucket) = a_bucket("copy").await;

    client
        .put_object()
        .bucket(&bucket)
        .key("staging/here")
        .body(ByteStream::from_static(b"copied bytes"))
        .send()
        .await
        .expect("el staging existe");

    client
        .copy_object()
        .bucket(&bucket)
        .key("blobs/final")
        .copy_source(format!("{bucket}/staging/here"))
        .send()
        .await
        .expect("la copia server-side sale");

    let got = client
        .get_object()
        .bucket(&bucket)
        .key("blobs/final")
        .send()
        .await
        .expect("el destino tiene los bytes");
    assert_eq!(got.body.collect().await.unwrap().to_vec(), b"copied bytes");
}

/// Doble publicacion del mismo contenido: en un CAS el ultimo escribe los
/// mismos bytes; lo que no puede pasar es que quede un intermedio con
/// otra longitud.
#[tokio::test]
async fn the_server_tolerates_double_publishing_of_same_key() {
    let (client, bucket) = a_bucket("double").await;

    let bytes = b"same content".to_vec();
    for _ in 0..2 {
        client
            .put_object()
            .bucket(&bucket)
            .key("blobs/same")
            .body(ByteStream::from(bytes.clone()))
            .send()
            .await
            .expect("publicar dos veces no es error");
    }
    let head = client
        .head_object()
        .bucket(&bucket)
        .key("blobs/same")
        .send()
        .await
        .expect("el objeto esta");
    assert_eq!(head.content_length.unwrap(), bytes.len() as i64);
}

/// El listado paginado con continuation token: la vista sobre la que el
/// reclaimer recorre el prefijo de blobs.
#[tokio::test]
async fn the_server_lists_with_paging() {
    let (client, bucket) = a_bucket("list").await;

    for i in 0..12u8 {
        client
            .put_object()
            .bucket(&bucket)
            .key(format!("blobs/{i:064x}"))
            .body(ByteStream::from_static(b"x"))
            .send()
            .await
            .expect("el blob esta");
    }
    let listed = client
        .list_objects_v2()
        .bucket(&bucket)
        .prefix("blobs/")
        .max_keys(5)
        .send()
        .await
        .expect("el listado sale");
    assert_eq!(
        listed.contents().len(),
        5,
        "la pagina es de a lo sumo max-keys"
    );
    let token = listed
        .next_continuation_token()
        .expect("hay mas pagina")
        .to_string();
    let more = client
        .list_objects_v2()
        .bucket(&bucket)
        .prefix("blobs/")
        .max_keys(50)
        .continuation_token(token)
        .send()
        .await
        .expect("la pagina siguiente sale");
    assert_eq!(listed.contents().len() + more.contents().len(), 12);
}

/// If-None-Match: la condicion de escritura condicional, que el backend
/// probea pero no usa. Lo que se afirma aqui es el CONTRATO de la
/// respuesta, no su bondad: si el servidor la soporta, publicar sobre un
/// objeto existente debe fallar con 412.
#[tokio::test]
async fn the_server_conditional_publishing_contract() {
    let (client, bucket) = a_bucket("conditional").await;

    let first = client
        .put_object()
        .bucket(&bucket)
        .key("blobs/cond")
        .if_none_match("*")
        .body(ByteStream::from_static(b"v1"))
        .send()
        .await;

    let second = client
        .put_object()
        .bucket(&bucket)
        .key("blobs/cond")
        .if_none_match("*")
        .body(ByteStream::from_static(b"v1"))
        .send()
        .await;

    if first.is_ok() {
        let error = second.expect_err("el condicional soportado tiene que rechazar el segundo");
        assert!(
            error.code() == Some("PreconditionFailed"),
            "el rechazo condicional es un 412: {error}"
        );
    } else {
        // El servidor no lo soporta: el backend no lo usara, y eso es
        // exactamente lo que el diseno declara (idempotencia del CAS).
        println!(
            "el servidor de pruebas no sirve If-None-Match; el backend vive de la idempotencia del CAS"
        );
    }
}
