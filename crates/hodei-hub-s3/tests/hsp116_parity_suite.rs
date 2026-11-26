//! The file backend passes the HAR-22 suite.

mod common;

use common::the_suite;
use hodei_hub::blob::{BlobStore, FileBlobStore};

fn a_file_store() -> (tempfile::TempDir, FileBlobStore) {
    let directory = tempfile::tempdir().expect("un directorio de usar y tirar");
    let store = FileBlobStore::new(directory.path()).expect("el almacen abre");
    (directory, store)
}

#[tokio::test]
async fn the_file_store_passes_the_suite() {
    let (_directory, store) = a_file_store();
    the_suite(&store).await;
    // The declared difference: the filesystem does have staging paths.
    let session = store.begin().await.expect("la sesion abre");
    assert!(session.the_staging_path().is_some());
}

#[tokio::test]
async fn the_in_memory_store_passes_the_suite_too() {
    // The fixture cannot be left outside the contract it claims to imitate.
    the_suite(&hodei_hub::blob::InMemoryBlobStore::empty()).await;
}

/// Restart: what one process wrote is served by the next, because the
/// directory is the state.
#[tokio::test]
async fn the_file_store_survives_a_restart() {
    let (directory, store) = a_file_store();
    let bytes = common::the_bytes();
    let digest = store.put(&bytes).await.expect("el contenido se guarda");
    drop(store);
    let reopened = FileBlobStore::new(directory.path()).expect("el almacen reopening abre");
    assert_eq!(
        reopened.get(&digest).await.expect("lo escrito revive"),
        bytes
    );
}
