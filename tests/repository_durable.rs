//! Durable repository adapter: restart and cross-writer uniqueness.

use std::sync::Arc;

use hodei_artifacts_api::repository::{
    RepositoryReader, RepositoryView, RepositoryWriteError, RepositoryWriter,
};
use hodei_artifacts_api::repository_surreal::SurrealRepositoryStore;
use kernel::Hrn;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

async fn open(path: &std::path::Path) -> Surreal<Db> {
    let db = Surreal::new::<RocksDb>(path).await.expect("rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    db
}

fn hrn() -> Hrn {
    Hrn::with_region(
        "hodei".to_string(),
        "artifacts".to_string(),
        "eu".to_string(),
        "acme".to_string(),
        "repository".to_string(),
        "releases".to_string(),
    )
}

fn view() -> RepositoryView {
    RepositoryView::new(
        hrn().to_string(),
        "acme".to_string(),
        "releases".to_string(),
        Some("durable".to_string()),
    )
}

#[tokio::test]
async fn create_is_persisted_in_the_database_and_duplicate_is_rejected() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("db");

    let db = open(&path).await;
    let store = SurrealRepositoryStore::new(db.clone());
    store.initialize().await.expect("initialize");
    store.create("acme", &hrn(), view()).await.expect("create");
    drop(store);

    // A new adapter over the same RocksDB database sees the row; the data is
    // not held in process memory.
    let store = SurrealRepositoryStore::new(db);
    store.initialize().await.expect("initialize");
    let found = store
        .find_by_hrn("acme", &hrn())
        .await
        .expect("read")
        .expect("repository survives restart");
    assert_eq!(found.name, "releases");
    assert_eq!(found.description.as_deref(), Some("durable"));

    assert_eq!(
        store.create("acme", &hrn(), view()).await.unwrap_err(),
        RepositoryWriteError::AlreadyExists
    );
}

#[tokio::test]
async fn two_writers_share_the_database_unique_index() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let path = tmp.path().join("db");
    let db = open(&path).await;
    let a = Arc::new(SurrealRepositoryStore::new(db.clone()));
    let b = Arc::new(SurrealRepositoryStore::new(db));
    a.initialize().await.unwrap();
    b.initialize().await.unwrap();

    let resource = hrn();
    let left = a.create("acme", &resource, view());
    let right = b.create("acme", &resource, view());
    let (left, right) = tokio::join!(left, right);

    let successes = [&left, &right]
        .iter()
        .filter(|result| result.is_ok())
        .count();
    let conflicts = [&left, &right]
        .iter()
        .filter(|result| matches!(result, Err(RepositoryWriteError::AlreadyExists)))
        .count();
    assert_eq!(successes, 1);
    assert_eq!(conflicts, 1);
}
