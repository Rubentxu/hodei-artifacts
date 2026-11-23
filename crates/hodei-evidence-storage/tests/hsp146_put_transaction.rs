//! The write of one piece of evidence has one frontier of commit.
//!
//! In SurrealDB every statement commits by itself: a batch that is not
//! wrapped in a transaction leaves a window between statements, and a
//! crash in that window leaves the external index naming a record that is
//! not there, or a record without the entries its subjects walk. The
//! store sends the batch of `put` wrapped in `BEGIN TRANSACTION` /
//! `COMMIT TRANSACTION`.
//!
//! The oracle here is two-fold: the statement the store sends -- it is
//! one transaction, and every upsert of the batch rides inside it --, and
//! the verdict of the engine over the real store -- the whole batch
//! lands, which proves the statement is accepted as it is sent.

use std::path::Path;
use std::sync::Arc;

use hodei_evidence::ports::EvidenceStore as _;
use hodei_evidence_storage::{SurrealEvidenceStore, declare_indexes, the_evidence_put_statement};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;

const ACCOUNT: &str = "acme";
const OPEN: &str = "BEGIN TRANSACTION;\n";
const CLOSE: &str = "COMMIT TRANSACTION;\n";

/// The statement the store sends for a batch of `subjects` pairs: it opens
/// its transaction first, closes it last, and nothing of the batch rides
/// outside.
#[test]
fn the_put_statement_wraps_the_whole_batch_in_one_transaction() {
    for subjects in 0..5 {
        let statement = the_evidence_put_statement(subjects);

        assert!(
            statement.starts_with(OPEN),
            "the batch must open its transaction first, got {statement:?}"
        );
        assert!(
            statement.ends_with(CLOSE),
            "the batch must close its transaction last, got {statement:?}"
        );

        let between = &statement[OPEN.len()..statement.len() - CLOSE.len()];

        // The record, its external index entry and one entry per subject:
        // every upsert of the batch is between the BEGIN and the COMMIT,
        // so a failure anywhere aborts the whole batch and nothing of it
        // is left behind.
        assert!(
            between.contains("UPSERT $id CONTENT $content;"),
            "the record of the evidence must ride inside the transaction, got {statement:?}"
        );
        assert!(
            between.contains("UPSERT $index_id CONTENT $index;"),
            "the external index entry must ride inside the transaction, got {statement:?}"
        );
        for position in 0..subjects {
            assert!(
                between.contains(&format!(
                    "UPSERT $pair_{position}_id CONTENT $pair_{position};"
                )),
                "the entry of subject {position} must ride inside the transaction, got {statement:?}"
            );
        }

        // The transaction opens once and closes once: no upsert of the
        // batch is split off into a commit of its own.
        assert!(
            !between.contains("COMMIT") && !between.contains("BEGIN"),
            "the transaction must not open or commit in the middle of the batch, got {statement:?}"
        );
    }
}

/// One record of evidence about two subjects, with the external identity
/// its producer gave it.
fn record(id: &str, subjects: Vec<kernel::Hrn>) -> hodei_evidence::EvidenceRecord {
    let tenant = kernel::Hrn::from_string(&format!("hrn:hodei:iam:eu:{ACCOUNT}:Artifact/{id}"))
        .expect("the tenant of the lab is a valid HRN");
    hodei_evidence::EvidenceRecord::builder(hodei_evidence::EvidenceId::new(id), tenant)
        .evidence_type(hodei_evidence::EvidenceType::Provenance)
        .subjects(subjects)
        .source(hodei_evidence::SourceRef::by_external_id(format!(
            "external-{id}"
        )))
        .content(hodei_evidence::ContentRef::by_digest(
            hodei_evidence::ContentDigest::of_bytes(id.as_bytes()),
        ))
        .observed_at(chrono::Utc::now())
        .recorded_at(chrono::Utc::now())
        .verification(hodei_evidence::EvidenceVerification::verified("signature"))
        .normalization(hodei_evidence::Normalization::new("perf-lab", "1.0.0"))
        .producer(hodei_evidence::Producer::new("connector", "perf-lab").with_version("1.0.0"))
        .build()
}

async fn open(path: &Path) -> Surreal<surrealdb::engine::local::Db> {
    let db = Surreal::new::<RocksDb>(path).await.expect("open rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    db
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hsp146-put-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// How many rows a table holds.
async fn rows(db: &Surreal<surrealdb::engine::local::Db>, table: &str) -> usize {
    let mut response = db
        .query(format!("SELECT count() FROM {table} GROUP ALL"))
        .await
        .expect("census")
        .check()
        .expect("census check");
    let census: Vec<std::collections::BTreeMap<String, serde_json::Value>> =
        response.take(0).expect("census rows");
    census
        .first()
        .and_then(|row| row.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

/// The whole batch lands, and lands once: the record, its external index
/// entry and one entry per subject, exactly the rows the batch names --
/// the statement the store sends is accepted by the engine as one
/// transaction, and nothing is lost or doubled on the way.
#[tokio::test]
async fn the_put_lands_every_row_of_its_batch() {
    let dir = scratch("batch");
    let db = open(&dir).await;
    declare_indexes(&db)
        .await
        .expect("the indexes are declared");
    let store = Arc::new(SurrealEvidenceStore::new(Arc::new(db.clone())));

    let first =
        kernel::Hrn::from_string(&format!("hrn:hodei:artifacts:eu:{ACCOUNT}:artifact/lab-0"))
            .expect("hrn");
    let second =
        kernel::Hrn::from_string(&format!("hrn:hodei:artifacts:eu:{ACCOUNT}:artifact/lab-1"))
            .expect("hrn");

    store
        .put(record("the-record", vec![first.clone(), second.clone()]))
        .await
        .expect("the batch of the record holds");

    assert_eq!(rows(&db, "evidence").await, 1, "one record of evidence");
    assert_eq!(
        rows(&db, "evidence_external").await,
        1,
        "one entry of the external index"
    );
    assert_eq!(
        rows(&db, "evidence_subject").await,
        2,
        "one entry per subject the record speaks about"
    );

    // The same record again: the same rows. The batch is upserts, so a
    // repetition rewrites what it names and nothing doubles.
    store
        .put(record("the-record", vec![first, second]))
        .await
        .expect("the same batch holds again");
    assert_eq!(rows(&db, "evidence").await, 1, "still one record");
    assert_eq!(rows(&db, "evidence_external").await, 1, "still one index");
    assert_eq!(rows(&db, "evidence_subject").await, 2, "still two pairs");

    // A record with no subjects: the record and its index entry ride in
    // the same transaction, and no pair is written.
    store
        .put(record("the-lonely-record", Vec::new()))
        .await
        .expect("the batch without subjects holds");
    assert_eq!(rows(&db, "evidence").await, 2, "the second record");
    assert_eq!(
        rows(&db, "evidence_external").await,
        2,
        "the index entry of the second record"
    );
    assert_eq!(
        rows(&db, "evidence_subject").await,
        2,
        "no pair for a record with no subjects"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
