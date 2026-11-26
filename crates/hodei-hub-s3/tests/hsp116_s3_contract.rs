//! The S3 backend passes the same suite as the file one, against a
//! real LocalStack over the localhost network.
//!
//! Besides the common suite, what is specific to the backend lives here: the
//! restart over the same store, the reclaimer that feeds the GC and the
//! promise that credentials do not leak.

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
    // The declared difference: S3 has no staging paths.
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
    // The same bucket, another store on top, as before the process died:
    // the object did not go away with whoever published it.
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

/// The reclaimer: what the GC sees. It lists the blobs by their digest with their
/// age and their length, respects the grace and tells what was deleted apart from what
/// never existed.
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

    // With a grace greater than any age, nothing is touched: it is the
    // window in which a recent publication is not an orphan.
    let fresh = reclaim
        .delete_if_older_than(&first, Duration::from_secs(60 * 60))
        .await
        .expect("la pregunta sale");
    assert_eq!(fresh, Reclaimed::Fresh, "la gracia manda antes de borrar");

    // Without grace, it is deleted and it says how much it freed.
    let deleted = reclaim
        .delete_if_older_than(&first, Duration::ZERO)
        .await
        .expect("el borrado sale");
    assert!(
        matches!(deleted, Reclaimed::Deleted { length } if length > 0),
        "el borrado informa de lo liberado: {deleted:?}"
    );
    assert_eq!(store.get(&first).await.unwrap_err(), BlobError::NotFound);

    // Deleting something that is no longer there is not an error: it is Gone, twice.
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

    // And the content that survived remains intact.
    assert!(store.exists(&second).await.expect("el segundo sigue"));
}

/// Safe provider credentials, with a sentinel value to
/// check that it does not leak.
const SENTINEL_SECRET: &str = "SENTINEL-do-not-log-this-key";

#[tokio::test]
async fn credentials_never_appear_in_debug_or_errors() {
    // The deployment builds the client with its provider chain;
    // here it is static with a sentinel, so it can be searched for afterwards.
    let credentials = Credentials::from_keys("AKIASENTINELID", SENTINEL_SECRET, None);
    let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(Region::new("us-east-1"))
        .credentials_provider(credentials)
        .load()
        .await;
    // A dead endpoint: all operations end in Unavailable,
    // which is exactly the error that must not carry secrets.
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
