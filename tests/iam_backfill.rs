//! Legacy IAM policy backfill: dry-run, copy, revoke and idempotence.

use std::collections::HashMap;

use hodei_artifacts_api::iam_backfill::{BackfillStatus, run};
use serde::Deserialize;
use serde_json::json;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

async fn open(path: &std::path::Path) -> Surreal<Db> {
    let db = Surreal::new::<RocksDb>(path).await.expect("rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    db
}

#[derive(Deserialize)]
struct Row {
    #[allow(dead_code)]
    content: String,
    #[serde(default)]
    #[allow(dead_code)]
    attached_principals: Vec<String>,
}

async fn exists(db: &Surreal<Db>, id: &str) -> bool {
    let row: Option<Row> = db.select(("policy", id)).await.unwrap();
    row.is_some()
}

#[tokio::test]
async fn dry_run_then_apply_copies_and_revokes_the_legacy_row() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    let _: Option<Row> = db
        .create(("policy", "legacy-a"))
        .content(json!({
            "content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::acme:User/alice"]
        }))
        .await
        .unwrap();

    let mapping = HashMap::new();
    let dry = run(&db, &mapping, false).await.unwrap();
    assert_eq!(dry.migrated, 0);
    assert_eq!(dry.entries.len(), 1);
    assert_eq!(dry.entries[0].status, BackfillStatus::Ready);
    assert!(exists(&db, "legacy-a").await);
    assert!(!exists(&db, "acme~legacy-a").await);

    let report = run(&db, &mapping, true).await.unwrap();
    assert_eq!(report.migrated, 1);
    assert!(exists(&db, "acme~legacy-a").await);
    assert!(!exists(&db, "legacy-a").await);

    // Idempotent: nothing is left to migrate.
    let again = run(&db, &mapping, true).await.unwrap();
    assert_eq!(again.migrated, 0);
    assert!(exists(&db, "acme~legacy-a").await);
}

#[tokio::test]
async fn ambiguous_attachments_are_not_migrated() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    let _: Option<Row> = db
        .create(("policy", "legacy-ambiguous"))
        .content(json!({
            "content": "permit(principal, action, resource);",
            "attached_principals": [
                "hrn:hodei:iam::acme:User/alice",
                "hrn:hodei:iam::other:User/bob"
            ]
        }))
        .await
        .unwrap();

    let report = run(&db, &HashMap::new(), true).await.unwrap();
    assert_eq!(report.migrated, 0);
    assert_eq!(report.ambiguous, 1);
    assert!(exists(&db, "legacy-ambiguous").await);
    assert!(!exists(&db, "acme~legacy-ambiguous").await);
}
