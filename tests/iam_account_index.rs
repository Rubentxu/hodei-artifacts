//! The account index has to exist on the deployment path.
//!
//! The IAM listing filters, counts and paginates by account inside SurrealDB,
//! and that query leans on the `policy_account` index. An index that no
//! deployment route creates is an index that no deployment has.
//!
//! The index is declared by a command of its own rather than by the service,
//! because defining an index on the local engine keeps the database directory
//! locked by the process that defined it: the service would serve correctly,
//! but the same process could never open the database again. The command
//! therefore runs while the service is not holding the database, which is the
//! ordinary order of a deployment step.

use std::path::Path;
use std::process::Command;

use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap};
use serde::Deserialize;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

#[derive(Default, Deserialize)]
struct TableInfo {
    #[serde(default)]
    indexes: std::collections::BTreeMap<String, String>,
}

/// The indexes the database declares for the `policy` table.
async fn policy_index_names(db: &Surreal<Db>) -> Vec<String> {
    let mut response = db
        .query("INFO FOR TABLE policy")
        .await
        .expect("the index listing must be readable");
    assert!(
        response.take_errors().is_empty(),
        "the index listing must not fail"
    );
    response
        .take::<Option<TableInfo>>(0)
        .ok()
        .flatten()
        .unwrap_or_default()
        .indexes
        .into_keys()
        .collect()
}

/// Run the operator command that declares the index.
fn run_ensure_index(db_path: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_ensure-iam-index"))
        .arg("--db")
        .arg(db_path)
        .output()
        .expect("the ensure-iam-index command must run")
}

/// Open a database that no other process holds.
///
/// Releasing the lock is asynchronous, so an open that arrives too early
/// fails with a lock error that says nothing about the index. That error —
/// and only that error — is retried inside a bounded window.
async fn open(path: &Path) -> Surreal<Db> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match Surreal::new::<RocksDb>(path).await {
            Ok(db) => {
                db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
                return db;
            }
            Err(error) => {
                let message = error.to_string();
                let lock_contention = message.contains("lock hold by current process")
                    || message.contains("No locks available");
                if !lock_contention || std::time::Instant::now() >= deadline {
                    panic!("the database must open: {error}");
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

#[tokio::test]
async fn the_operator_command_creates_the_account_index() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("db");

    let output = run_ensure_index(&db_path);
    assert!(
        output.status.success(),
        "the command must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let db = open(&db_path).await;
    let names = policy_index_names(&db).await;
    assert!(
        names.iter().any(|name| name == "policy_account"),
        "the command must leave the `policy_account` index in place, found {names:?}"
    );
}

#[tokio::test]
async fn the_operator_command_is_idempotent() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("db");

    for run in 1..=2 {
        let output = run_ensure_index(&db_path);
        assert!(
            output.status.success(),
            "run {run} of the command must succeed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let db = open(&db_path).await;
    let names = policy_index_names(&db).await;
    assert!(
        names.iter().any(|name| name == "policy_account"),
        "the index must be in place after running the command twice, found {names:?}"
    );
}

#[tokio::test]
async fn the_deployment_path_declares_the_index_before_the_service_starts() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("db");
    let mut config = AppConfig::default();
    config.rocksdb.path = db_path.to_string_lossy().into_owned();

    // The deployment step: the index is declared before the service starts.
    let output = run_ensure_index(&db_path);
    assert!(
        output.status.success(),
        "the command must succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // The service then boots against the indexed database.
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("the bootstrap must succeed");
    drop(state);

    let db = open(&db_path).await;
    let names = policy_index_names(&db).await;
    assert!(
        names.iter().any(|name| name == "policy_account"),
        "the index must survive the service booting, found {names:?}"
    );
}

#[tokio::test]
async fn the_service_starts_without_the_account_index() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();

    // A missing index costs the listing its speed, not its isolation, so the
    // service starts anyway and reports the absence.
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("the bootstrap must succeed without the index");
    drop(state);
}
