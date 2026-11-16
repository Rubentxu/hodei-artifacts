//! El contrato de recuperacion de bytes del CAS en disco.
//!
//! Lo que garantiza: se lista solo lo que es un blob; el borrado respeta
//! la edad; reutilizar un blob (dedup) renueva su edad y por tanto lo
//! protege; y borrar dos veces no falla ni borra nada mas.

use std::time::{Duration, SystemTime};

use hodei_evidence::ContentDigest;
use hodei_hub::{BlobReclaim, BlobStore, FileBlobStore, Reclaimed};

fn age_the_blob(store: &FileBlobStore, digest: &ContentDigest, by: Duration) {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(store.the_path_of(digest))
        .expect("the blob opens");
    file.set_modified(SystemTime::now() - by)
        .expect("the date is set");
}

#[tokio::test]
async fn the_listing_has_only_blobs_with_their_length_and_age() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FileBlobStore::new(dir.path()).expect("store");
    let digest = store.put(b"alpha").await.expect("put");
    // Cosas que no son blobs: no se listan.
    std::fs::write(dir.path().join("README"), b"x").expect("write");
    std::fs::create_dir_all(dir.path().join("zz/yy")).expect("dirs");
    std::fs::write(dir.path().join("zz/yy/not-a-digest"), b"x").expect("write");
    let session = store.begin().await.expect("session");

    let blobs = store.list_blobs().await.expect("list");
    assert_eq!(blobs.len(), 1, "solo el blob: {blobs:?}");
    assert_eq!(blobs[0].digest, digest);
    assert_eq!(blobs[0].length, 5);
    drop(session);
}

#[tokio::test]
async fn a_blob_inside_the_grace_is_kept_and_an_old_one_is_deleted_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FileBlobStore::new(dir.path()).expect("store");
    let digest = store.put(b"beta").await.expect("put");
    let grace = Duration::from_secs(3600);

    assert_eq!(
        store
            .delete_if_older_than(&digest, grace)
            .await
            .expect("ok"),
        Reclaimed::Fresh
    );
    assert!(store.exists(&digest).await.expect("exists"));

    age_the_blob(&store, &digest, Duration::from_secs(7200));
    assert_eq!(
        store
            .delete_if_older_than(&digest, grace)
            .await
            .expect("ok"),
        Reclaimed::Deleted { length: 4 }
    );
    assert!(!store.exists(&digest).await.expect("exists"));
    // Segunda vez: ya no esta, y no es un error.
    assert_eq!(
        store
            .delete_if_older_than(&digest, grace)
            .await
            .expect("ok"),
        Reclaimed::Gone
    );
}

#[tokio::test]
async fn reusing_a_blob_renews_its_age_so_the_collector_cannot_take_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FileBlobStore::new(dir.path()).expect("store");
    let digest = store.put(b"gamma").await.expect("put");
    age_the_blob(&store, &digest, Duration::from_secs(7200));
    let grace = Duration::from_secs(3600);

    // Un `put` del mismo contenido reutiliza el blob...
    store.put(b"gamma").await.expect("dedup put");
    assert_eq!(
        store
            .delete_if_older_than(&digest, grace)
            .await
            .expect("ok"),
        Reclaimed::Fresh,
        "el put que reutiliza el blob renueva su edad"
    );

    // ...y un commit de sesion tambien.
    age_the_blob(&store, &digest, Duration::from_secs(7200));
    let mut session = store.begin().await.expect("session");
    session.append(b"gamma").await.expect("append");
    session
        .commit(hodei_hub::CommitExpectation {
            digest: digest.clone(),
            length: 5,
        })
        .await
        .expect("commit");
    assert_eq!(
        store
            .delete_if_older_than(&digest, grace)
            .await
            .expect("ok"),
        Reclaimed::Fresh,
        "el commit que reutiliza el blob renueva su edad"
    );
}

#[tokio::test]
async fn a_concurrent_publisher_and_collector_never_lose_the_blob() {
    // Mil carreras: el recolector borra "lo viejo" mientras alguien
    // vuelve a subir el mismo contenido. Al acabar, si el put dijo que
    // estaba, el blob esta.
    let dir = tempfile::tempdir().expect("tempdir");
    let store = std::sync::Arc::new(FileBlobStore::new(dir.path()).expect("store"));
    for round in 0..200 {
        let bytes = format!("race-{round}").into_bytes();
        let digest = store.put(&bytes).await.expect("put");
        age_the_blob(&store, &digest, Duration::from_secs(7200));
        let collector = {
            let store = store.clone();
            let digest = digest.clone();
            tokio::spawn(async move {
                store
                    .delete_if_older_than(&digest, Duration::from_secs(3600))
                    .await
            })
        };
        let publisher = {
            let store = store.clone();
            let bytes = bytes.clone();
            tokio::spawn(async move { store.put(&bytes).await })
        };
        let _ = collector.await.expect("join").expect("collector");
        let stored = publisher.await.expect("join").expect("publisher");
        assert_eq!(stored, digest);
        assert!(
            store.exists(&digest).await.expect("exists"),
            "ronda {round}: el put confirmo el blob y no esta"
        );
    }
}
