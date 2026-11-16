//! El contrato del libro de recibos de procedencia contra SurrealDB de verdad.
//!
//! Lo que se comprueba: un recibo se lee tal y como se escribio, repetirlo no
//! lo reescribe, el mas reciente va primero, cuentas, repositorios y rutas no
//! se mezclan, y todo sobrevive a reabrir la base.

use std::sync::Arc;

use chrono::{Duration, Utc};
use hodei_catalog::{AllowAllAuthorizer, ChecksumEvidence, ProvenanceLedger, ProvenanceReceipt};
use hodei_catalog_storage::SurrealCatalogStore;
use hodei_evidence::ContentDigest;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

async fn open(path: &std::path::Path) -> Surreal<Db> {
    let mut last = String::new();
    for _ in 0..20 {
        match Surreal::new::<RocksDb>(path).await {
            Ok(db) => {
                db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
                return db;
            }
            Err(error) => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
    panic!("the database could not be opened: {last}");
}

fn store(db: Surreal<Db>) -> SurrealCatalogStore {
    SurrealCatalogStore::with_authorizer(db, Arc::new(AllowAllAuthorizer))
}

fn receipt(
    id: &str,
    account: &str,
    repository: &str,
    path: &str,
    age_secs: i64,
) -> ProvenanceReceipt {
    ProvenanceReceipt {
        id: id.to_string(),
        account: account.to_string(),
        repository: repository.to_string(),
        asset_path: path.to_string(),
        upstream_id: "central".to_string(),
        source_url: "https://repo.example.org/raw/tool.bin".to_string(),
        config_version: 3,
        digest: ContentDigest::of_bytes(id.as_bytes()),
        size_bytes: 1234,
        checksum: ChecksumEvidence::Declared {
            header: "x-checksum-sha256".to_string(),
        },
        status: 200,
        redirects: 1,
        etag: Some("\"abc\"".to_string()),
        actor: "hrn:hodei:iam:acme:User/alice".to_string(),
        correlation_id: format!("corr-{id}"),
        fetched_at: Utc::now() - Duration::seconds(age_secs),
    }
}

async fn fresh() -> (tempfile::TempDir, SurrealCatalogStore) {
    let tmp = tempfile::tempdir().expect("tmp");
    let store = store(open(&tmp.path().join("db")).await);
    store.initialize().await.expect("initialize");
    (tmp, store)
}

#[tokio::test]
async fn a_receipt_is_read_as_it_was_written() {
    let (_tmp, store) = fresh().await;
    let written = receipt("r-1", "acme", "mirror", "tools/a.bin", 0);
    store.record(&written).await.expect("record");

    let found = store
        .receipts_of("acme", "mirror", "tools/a.bin")
        .await
        .expect("read");
    assert_eq!(found.len(), 1);
    let found = &found[0];
    assert_eq!(found.id, written.id);
    assert_eq!(found.digest, written.digest);
    assert_eq!(found.size_bytes, 1234);
    assert_eq!(found.checksum, written.checksum);
    assert_eq!(found.upstream_id, "central");
    assert_eq!(found.source_url, written.source_url);
    assert_eq!(found.config_version, 3);
    assert_eq!(found.redirects, 1);
    assert_eq!(found.status, 200);
    assert_eq!(found.etag, written.etag);
    assert_eq!(found.actor, written.actor);
    assert_eq!(found.correlation_id, "corr-r-1");
}

#[tokio::test]
async fn a_checksum_computed_only_is_told_apart_from_a_declared_one() {
    let (_tmp, store) = fresh().await;
    let mut computed = receipt("r-1", "acme", "mirror", "a", 0);
    computed.checksum = ChecksumEvidence::ComputedOnly;
    computed.etag = None;
    store.record(&computed).await.expect("record");

    let found = store
        .receipts_of("acme", "mirror", "a")
        .await
        .expect("read");
    assert_eq!(found[0].checksum, ChecksumEvidence::ComputedOnly);
    assert!(found[0].etag.is_none());
}

#[tokio::test]
async fn recording_the_same_receipt_twice_leaves_it_as_it_was() {
    let (_tmp, store) = fresh().await;
    let first = receipt("r-1", "acme", "mirror", "a", 10);
    store.record(&first).await.expect("record");

    let mut again = first.clone();
    again.size_bytes = 9;
    again.upstream_id = "other".to_string();
    store.record(&again).await.expect("record again");

    let found = store
        .receipts_of("acme", "mirror", "a")
        .await
        .expect("read");
    assert_eq!(found.len(), 1, "no se duplica");
    assert_eq!(found[0].size_bytes, 1234, "lo escrito no se reescribe");
    assert_eq!(found[0].upstream_id, "central");
}

#[tokio::test]
async fn the_newest_receipt_comes_first() {
    let (_tmp, store) = fresh().await;
    store
        .record(&receipt("old", "acme", "mirror", "a", 300))
        .await
        .expect("old");
    store
        .record(&receipt("new", "acme", "mirror", "a", 5))
        .await
        .expect("new");
    store
        .record(&receipt("mid", "acme", "mirror", "a", 60))
        .await
        .expect("mid");

    let ids: Vec<String> = store
        .receipts_of("acme", "mirror", "a")
        .await
        .expect("read")
        .into_iter()
        .map(|receipt| receipt.id)
        .collect();
    assert_eq!(ids, ["new", "mid", "old"]);
}

#[tokio::test]
async fn accounts_repositories_and_paths_do_not_mix() {
    let (_tmp, store) = fresh().await;
    store
        .record(&receipt("r-a", "acme", "mirror", "a", 0))
        .await
        .expect("a");
    store
        .record(&receipt("r-b", "acme", "mirror", "b", 0))
        .await
        .expect("b");
    store
        .record(&receipt("r-m", "acme", "other", "a", 0))
        .await
        .expect("other repo");
    store
        .record(&receipt("r-x", "umbrella", "mirror", "a", 0))
        .await
        .expect("other account");

    for (account, repository, path, expected) in [
        ("acme", "mirror", "a", "r-a"),
        ("acme", "mirror", "b", "r-b"),
        ("acme", "other", "a", "r-m"),
        ("umbrella", "mirror", "a", "r-x"),
    ] {
        let found = store
            .receipts_of(account, repository, path)
            .await
            .expect("read");
        assert_eq!(found.len(), 1, "{account}/{repository}/{path}");
        assert_eq!(found[0].id, expected);
    }
    assert!(
        store
            .receipts_of("acme", "mirror", "missing")
            .await
            .expect("read")
            .is_empty()
    );
}

const RESTART_PATH: &str = "HODEI_HSP106_RESTART_DB";

#[tokio::test]
#[ignore = "solo corre como el proceso hijo del test de reinicio"]
async fn the_receipts_are_written_for_the_restart() {
    let path = std::env::var(RESTART_PATH).expect("the path of the database");
    let store = store(open(std::path::Path::new(&path)).await);
    store.initialize().await.expect("initialize");
    store
        .record(&receipt("r-1", "acme", "mirror", "a", 0))
        .await
        .expect("record");
}

#[tokio::test]
async fn receipts_survive_a_restart() {
    let tmp = tempfile::tempdir().expect("tmp");
    let path = tmp.path().join("db");
    // RocksDB bloquea la base dentro del proceso: quien escribe es otro.
    let status = tokio::process::Command::new(std::env::current_exe().expect("the test binary"))
        .arg("--exact")
        .arg("the_receipts_are_written_for_the_restart")
        .arg("--ignored")
        .arg("--nocapture")
        .env(RESTART_PATH, path.to_string_lossy().as_ref())
        .status()
        .await
        .expect("the writing process");
    assert!(status.success(), "the writing process failed: {status}");

    let store = store(open(&path).await);
    store.initialize().await.expect("initialize again");
    let found = store
        .receipts_of("acme", "mirror", "a")
        .await
        .expect("read");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, "r-1");
}
