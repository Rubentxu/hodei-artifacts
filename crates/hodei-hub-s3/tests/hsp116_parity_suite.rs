//! El backend de ficheros pasa la suite de HAR-22.

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
    // La diferencia declarada: el filesystem si tiene caminos de staging.
    let session = store.begin().await.expect("la sesion abre");
    assert!(session.the_staging_path().is_some());
}

#[tokio::test]
async fn the_in_memory_store_passes_the_suite_too() {
    // El fixture no puede quedar fuera del contrato que dice imitar.
    the_suite(&hodei_hub::blob::InMemoryBlobStore::empty()).await;
}

/// Restart: lo escrito por un proceso lo sirve el siguiente, porque el
/// directorio es el estado.
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
