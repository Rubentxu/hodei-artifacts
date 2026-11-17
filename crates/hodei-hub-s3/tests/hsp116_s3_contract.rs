//! El backend S3 pasa la misma suite que el de ficheros, contra un
//! LocalStack real por red localhost.
//!
//! Ademas de la suite comun, aqui viven lo que es propio del backend: el
//! restart sobre el mismo almacén, el reclaimer que alimenta al GC y la
//! promesa de que las credenciales no se filtran.

mod common;

use std::time::Duration;

use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::Region;
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobError, BlobStore};
use hodei_hub::reclaim::Reclaimed;
use hodei_hub_s3::{S3BlobConfig, S3BlobConfigError, S3BlobStore};

#[tokio::test]
async fn the_s3_store_passes_the_suite() {
    let bucket = common::localstack::a_unique_bucket("suite");
    let store = common::localstack::a_store(&bucket).await;
    common::the_suite(&store).await;
    // La diferencia declarada: S3 no tiene caminos de staging.
    let session = store.begin().await.expect("la sesion abre");
    assert!(session.the_staging_path().is_none());
}

#[tokio::test]
async fn the_s3_store_survives_a_restart() {
    let bucket = common::localstack::a_unique_bucket("restart");
    let store = common::localstack::a_store(&bucket).await;
    let bytes = common::the_bytes();
    let digest = store.put(&bytes).await.expect("el contenido se guarda");
    drop(store);
    // El mismo cubo, otro almacen encima, como el de antes de morir el
    // proceso: el objeto no se fue con quien lo publico.
    let reopened = common::localstack::a_store(&bucket).await;
    assert_eq!(
        reopened.get(&digest).await.expect("lo escrito revive"),
        bytes
    );
    reopened
        .verify(&digest)
        .await
        .expect("y sigue siendo integro");
}

/// El reclaimer: lo que ve el GC. Lista los blobs por su digest con su
/// edad y su longitud, respeta la gracia y distingue lo borrado de lo que
/// nunca estuvo.
#[tokio::test]
async fn the_reclaimer_shows_the_gc_a_consistent_view() {
    let bucket = common::localstack::a_unique_bucket("gc");
    let store = common::localstack::a_store(&bucket).await;

    let first = store
        .put(b"the first orphan".to_vec().as_slice())
        .await
        .expect("un blob se guarda");
    let second = store
        .put(b"the second orphan".to_vec().as_slice())
        .await
        .expect("otro blob se guarda");

    let reclaim = store.reclaimer().expect("el almacen sabe recolectarse");
    let mut blobs = reclaim.list_blobs().await.expect("el listado sale");
    let mut digests: Vec<_> = blobs.iter().map(|blob| blob.digest.clone()).collect();
    digests.sort_by(|a, b| a.value.cmp(&b.value));
    blobs.sort_by(|a, b| a.digest.value.cmp(&b.digest.value));
    let mut expected = vec![first.clone(), second.clone()];
    expected.sort_by(|a, b| a.value.cmp(&b.value));
    assert_eq!(
        digests, expected,
        "el listado trae exactamente los dos blobs"
    );
    for blob in &blobs {
        assert!(blob.length > 0, "cada blob trae su longitud");
        let _ = blob.age;
    }

    // Con una gracia mayor que cualquier edad, nada se toca: es la
    // ventana en la que una publicacion reciente no es huerfana.
    let fresh = reclaim
        .delete_if_older_than(&first, Duration::from_secs(60 * 60))
        .await
        .expect("la pregunta sale");
    assert_eq!(fresh, Reclaimed::Fresh, "la gracia manda antes de borrar");

    // Sin gracia, se borra y se dice cuanto liberaba.
    let deleted = reclaim
        .delete_if_older_than(&first, Duration::ZERO)
        .await
        .expect("el borrado sale");
    assert!(
        matches!(deleted, Reclaimed::Deleted { length } if length > 0),
        "el borrado informa de lo liberado: {deleted:?}"
    );
    assert_eq!(store.get(&first).await.unwrap_err(), BlobError::NotFound);

    // Borrar algo que ya no esta no es un error: es Gone, dos veces.
    let again = reclaim
        .delete_if_older_than(&first, Duration::ZERO)
        .await
        .expect("la repeticion sale");
    assert_eq!(again, Reclaimed::Gone);
    let absent = reclaim
        .delete_if_older_than(&ContentDigest::of_bytes(b"never existed"), Duration::ZERO)
        .await
        .expect("lo ausente sale");
    assert_eq!(absent, Reclaimed::Gone);

    // Y el contenido que sobrevivio sigue intacto.
    assert!(store.exists(&second).await.expect("el segundo sigue"));
}

/// Credenciales de proveedor seguro, con un valor centinela para
/// comprobar que no se filtra.
const SENTINEL_SECRET: &str = "SENTINEL-do-not-log-this-key";

#[tokio::test]
async fn credentials_never_appear_in_debug_or_errors() {
    // El despliegue construye el cliente con su cadena de proveedores;
    // aqui es estatica con un centinela, para poder buscarlo despues.
    let credentials = Credentials::from_keys("AKIASENTINELID", SENTINEL_SECRET, None);
    let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;
    // Un endpoint muerto: todas las operaciones acaban en Unavailable,
    // que es exactamente el error que no debe llevar secretos.
    let s3_config = aws_sdk_s3::config::Builder::from(&sdk)
        .endpoint_url("http://127.0.0.1:1")
        .force_path_style(true)
        .build();
    let client = Client::from_conf(s3_config);
    let store = S3BlobStore::new(client, S3BlobConfig::new("hsp116-leak")).expect("abre");

    let shown = format!("{store:?}");
    assert!(
        !shown.contains(SENTINEL_SECRET),
        "el Debug no lleva el secreto"
    );

    let error = store
        .put(b"anything".as_slice())
        .await
        .expect_err("el almacen muerto no sirve");
    let described = error.to_string();
    assert!(
        matches!(error, BlobError::Unavailable(_)),
        "el fallo es de disponibilidad: {error}"
    );
    assert!(
        !described.contains(SENTINEL_SECRET),
        "el error no filtra el secreto: {described}"
    );
}

#[tokio::test]
async fn a_part_size_below_the_s3_minimum_is_refused_at_construction() {
    let bucket = common::localstack::a_unique_bucket("partsize");
    let client = common::localstack::a_client().await;
    let small = S3BlobConfig {
        bucket,
        part_size: 1024,
        max_inflight_parts: 2,
    };
    let error = S3BlobStore::new(client, small).expect_err("S3 exige 5 MiB por trozo");
    assert_eq!(
        error,
        S3BlobConfigError::PartTooSmall {
            given: 1024,
            minimum: 5 * 1024 * 1024
        }
    );
}
