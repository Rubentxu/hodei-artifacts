//! The read of the evidence of one subject is bounded by the subject it
//! names, and its cost does not grow with the size of the evidence.
//!
//! The trust capsule reads the evidence of a subject. The read used to
//! ask the evidence table a containment over its array of subjects
//! (`subjects CONTAINS $subject`), and the planner cannot bind a
//! containment into an index read: it binds the equality of the account
//! and filters the containment row by row, so the capsule read grew with
//! the size of the evidence -- measured at 211 ms per read with 100k
//! records and 16.1 s with 1M, whatever the subject held. The store now
//! answers the question from a table of subject entries, written next to
//! the record in the same transaction, and the read is two reads: the
//! entries by equality on the account and the subject (both bound into
//! the index), then the records they name by identity.
//!
//! The oracle here is the query plan, not the clock: a plan that binds
//! the account and the subject into the index read cannot grow with the
//! evidence, whatever the hardware.

use std::path::Path;
use std::sync::Arc;

use hodei_evidence::ports::EvidenceStore as _;
use hodei_evidence_storage::{
    EVIDENCE_SUBJECT_TABLE, SurrealEvidenceStore, declare_indexes, the_subject_pair_read,
    the_subject_records_read,
};
use surrealdb::Surreal;
use surrealdb::engine::local::RocksDb;

const ACCOUNT: &str = "acme";
const OTHER_ACCOUNT: &str = "other";
/// Distractor rows the test seeds, so the read runs against evidence the
/// subject never names. The plan does not depend on the count; the count
/// only makes the contrast with a seeded table explicit.
const DISTRACTOR_ROWS: usize = 2_000;

fn subject_hrn(account: &str, index: usize) -> String {
    format!("hrn:hodei:artifacts:eu:{account}:artifact/lab-{index}")
}

async fn open(path: &Path) -> Surreal<surrealdb::engine::local::Db> {
    let db = Surreal::new::<RocksDb>(path).await.expect("open rocksdb");
    db.use_ns("hodei").use_db("artifacts").await.expect("ns/db");
    db
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("hsp176-subject-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// One record of evidence about a subject, written through the store.
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

/// Evidence rows seeded like the perf-lab profile does: bulk records in
/// subjects the read never names. The pairs they carry are seeded with
/// them, so the pair table holds distractor entries the read must walk
/// past by index and not by filtering.
#[derive(Debug, serde::Serialize)]
struct DistractorRow {
    evidence_id: String,
    tenant_hrn: String,
    producer_id: String,
    external_id: Option<String>,
    evidence_type: String,
    content_digest: String,
    content: serde_json::Value,
    observed_at: surrealdb::sql::Datetime,
    recorded_at: surrealdb::sql::Datetime,
    verification_status: String,
    account: String,
    subjects: Vec<String>,
    record: serde_json::Value,
}

#[derive(Debug, serde::Serialize)]
struct DistractorPair {
    account: String,
    subject: String,
    evidence_id: String,
}

async fn seed_distractor(db: &Surreal<surrealdb::engine::local::Db>, rows: usize) {
    for batch_start in (0..rows).step_by(1_000) {
        let batch_end = (batch_start + 1_000).min(rows);
        let batch: Vec<DistractorRow> = (batch_start..batch_end)
            .map(|index| {
                let subject = subject_hrn(ACCOUNT, 1 + index % 49);
                DistractorRow {
                    evidence_id: format!("ev-{index}"),
                    tenant_hrn: format!("hrn:hodei:iam:eu:{ACCOUNT}:User/perf"),
                    producer_id: "connector:perf-lab".into(),
                    external_id: Some(format!("perf-evd-{index}")),
                    evidence_type: "Provenance".into(),
                    content_digest: format!("sha256:{:064x}", index),
                    content: serde_json::json!({
                        "digest": format!("sha256:{:064x}", index),
                    }),
                    observed_at: surrealdb::sql::Datetime::from(chrono::Utc::now()),
                    recorded_at: surrealdb::sql::Datetime::from(chrono::Utc::now()),
                    verification_status: "Verified".into(),
                    account: ACCOUNT.into(),
                    subjects: vec![subject.clone()],
                    record: serde_json::json!({"seed": index}),
                }
            })
            .collect();
        db.query("INSERT INTO evidence $rows")
            .bind(("rows", batch))
            .await
            .expect("insert distractor")
            .check()
            .expect("insert distractor check");
        let pairs: Vec<DistractorPair> = (batch_start..batch_end)
            .map(|index| DistractorPair {
                account: ACCOUNT.into(),
                subject: subject_hrn(ACCOUNT, 1 + index % 49),
                evidence_id: format!("ev-{index}"),
            })
            .collect();
        db.query("INSERT INTO evidence_subject $rows")
            .bind(("rows", pairs))
            .await
            .expect("insert distractor pairs")
            .check()
            .expect("insert distractor pairs check");
    }
}

/// One step of a query plan: the operation the planner picked. The rest
/// of the step is ignored on purpose: the detail of a plan that carries
/// record identities does not always come back as JSON, and the
/// operation is the part the boundedness rides on.
#[derive(Debug, serde::Deserialize)]
struct PlanStep {
    operation: String,
}

/// The plan the planner builds for a statement, against whatever the
/// tables currently hold.
async fn plan_of(
    db: &Surreal<surrealdb::engine::local::Db>,
    select: &str,
    binds: &[(&str, serde_json::Value)],
) -> Vec<PlanStep> {
    let mut statement = db.query(format!("{select} EXPLAIN"));
    for (name, value) in binds {
        statement = statement.bind((name.to_string(), value.clone()));
    }
    let mut response = statement
        .await
        .expect("explain")
        .check()
        .expect("explain check");
    response.take(0).expect("explain rows")
}

/// The binds of the pair read, for the subject the tests ask about.
fn pair_binds(subject: &str) -> Vec<(&'static str, serde_json::Value)> {
    vec![
        (
            "table",
            serde_json::Value::String(EVIDENCE_SUBJECT_TABLE.into()),
        ),
        ("account", serde_json::Value::String(ACCOUNT.into())),
        ("subject", serde_json::Value::String(subject.into())),
    ]
}

/// The full detail of one plan step, when the planner hands it back.
#[derive(Debug, serde::Deserialize)]
struct DetailStep {
    operation: String,
    #[serde(default)]
    detail: serde_json::Value,
}

/// The plan of the pair read with the details the planner does hand back:
/// the binding of the index read, which says which predicates it carries.
async fn detail_of(
    db: &Surreal<surrealdb::engine::local::Db>,
    select: &str,
    binds: &[(&str, serde_json::Value)],
) -> Vec<DetailStep> {
    let mut statement = db.query(format!("{select} EXPLAIN FULL"));
    for (name, value) in binds {
        statement = statement.bind((name.to_string(), value.clone()));
    }
    let mut response = statement
        .await
        .expect("explain full")
        .check()
        .expect("explain full check");
    response.take(0).expect("explain full rows")
}

/// How many rows each table holds: the evidence and its subject entries.
async fn row_census(db: &Surreal<surrealdb::engine::local::Db>) -> (usize, usize) {
    async fn count(db: &Surreal<surrealdb::engine::local::Db>, table: &str) -> usize {
        let mut response = db
            .query(format!("SELECT count() FROM {table} GROUP ALL"))
            .await
            .expect("census")
            .check()
            .expect("census check");
        let rows: Vec<std::collections::BTreeMap<String, serde_json::Value>> =
            response.take(0).expect("census rows");
        rows.first()
            .and_then(|row| row.get("count"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as usize
    }
    let evidence = count(db, "evidence").await;
    let pairs = count(db, EVIDENCE_SUBJECT_TABLE).await;
    (evidence, pairs)
}

/// The subject read must plan a bounded index read that does not grow
/// with the evidence, and the containment shape it replaced must plan a
/// table scan: that scan is the shape the read must never carry again.
#[tokio::test]
async fn the_subject_read_plan_does_not_grow_with_the_evidence() {
    let dir = scratch("plan");
    let db = open(&dir).await;
    declare_indexes(&db)
        .await
        .expect("the indexes are declared");

    let subject = subject_hrn(ACCOUNT, 0);

    // With the tables empty, the read of the entries plans the index.
    let empty_plan = plan_of(&db, the_subject_pair_read(), &pair_binds(&subject)).await;
    let empty_operations: Vec<&str> = empty_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        empty_operations.contains(&"Iterate Index"),
        "the pair read must plan a read of the index, got {empty_operations:?}"
    );
    assert!(
        !empty_operations.contains(&"Iterate Table"),
        "the pair read must not plan a table scan, got {empty_operations:?}"
    );

    // Seed the distractor: the evidence table and the pair table now hold
    // rows the subject never names. The plan of the read must not move.
    seed_distractor(&db, DISTRACTOR_ROWS).await;
    let seeded_plan = plan_of(&db, the_subject_pair_read(), &pair_binds(&subject)).await;
    let seeded_operations: Vec<&str> = seeded_plan.iter().map(|s| s.operation.as_str()).collect();
    assert_eq!(
        empty_operations, seeded_operations,
        "the plan of the pair read must not depend on the size of the evidence"
    );

    // The binding of the index read carries the account and the subject
    // the read asks for, both of them: no predicate is left to filter by.
    let steps = detail_of(&db, the_subject_pair_read(), &pair_binds(&subject)).await;
    let binding = steps
        .iter()
        .find(|step| step.operation == "Iterate Index")
        .and_then(|step| step.detail.get("plan"))
        .expect("the index read names its binding");
    assert_eq!(
        binding["operator"], "=",
        "the index read is bound by equality, got {binding}"
    );
    let bound: Vec<&str> = binding["value"]
        .as_array()
        .expect("the binding carries both values")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(
        bound.contains(&ACCOUNT) && bound.contains(&subject.as_str()),
        "the index read must bind the account and the subject, got {bound:?}"
    );

    // The fetch of the records the entries name is a fetch by identity,
    // not a scan of the evidence table. The identifiers are bound the
    // same way the store binds them: record identities, and no table to
    // walk.
    let mut response = db
        .query(format!("{} EXPLAIN", the_subject_records_read()))
        .bind((
            "ids",
            vec![surrealdb::sql::Thing::from((
                "evidence".to_string(),
                "no-such-row".to_string(),
            ))],
        ))
        .await
        .expect("explain the fetch")
        .check()
        .expect("explain the fetch check");
    let fetch_plan: Vec<PlanStep> = response.take(0).expect("fetch plan rows");
    let fetch_operations: Vec<&str> = fetch_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        !fetch_operations.contains(&"Iterate Table"),
        "the fetch of the named records must not scan the table, got {fetch_operations:?}"
    );

    // The banned shape, for contrast: the containment over the array the
    // read used to ask, over the same tables and the same bindings -- and
    // the planner answers with a full-table scan, because a containment
    // cannot be bound into an index read. This is the shape the capsule
    // read must never carry again.
    let banned_plan = plan_of(
        &db,
        "SELECT * FROM type::table($table) WHERE account = $account AND subjects CONTAINS $subject",
        &[
            ("table", serde_json::json!("evidence")),
            ("account", serde_json::json!(ACCOUNT)),
            ("subject", serde_json::json!(subject)),
        ],
    )
    .await;
    let banned_operations: Vec<&str> = banned_plan.iter().map(|s| s.operation.as_str()).collect();
    assert!(
        banned_operations.contains(&"Iterate Table"),
        "the banned containment shape must plan a table scan, got {banned_operations:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The read through the port returns exactly the evidence the subject
/// holds: the same records the containment shape read, and nothing the
/// subject does not name -- not the distractors, not another account's
/// rows, and not a row that cannot be read back as evidence.
#[tokio::test]
async fn the_subject_read_returns_exactly_the_evidence_of_the_subject() {
    let dir = scratch("verdict");
    let db = open(&dir).await;
    declare_indexes(&db)
        .await
        .expect("the indexes are declared");
    let store = Arc::new(SurrealEvidenceStore::new(Arc::new(db.clone())));

    let subject = kernel::Hrn::from_string(&subject_hrn(ACCOUNT, 0)).expect("hrn");

    // Three records of the subject, one of them speaking about a second
    // subject too: the read must hand it back once, not once per subject.
    for index in 0..3 {
        let subjects = if index == 2 {
            vec![
                subject.clone(),
                kernel::Hrn::from_string(&subject_hrn(ACCOUNT, 99)).expect("hrn"),
            ]
        } else {
            vec![subject.clone()]
        };
        store
            .put(record(&format!("store-ev-{index}"), subjects))
            .await
            .expect("the record of the subject exists");
    }
    seed_distractor(&db, DISTRACTOR_ROWS).await;

    // A record of another account, carrying the same subject: the account
    // is part of the question, and this row must never come back to the
    // reader of the lab account.
    let poisoned = serde_json::json!({
        "evidence_id": "poisoned-ev",
        "tenant_hrn": format!("hrn:hodei:iam:eu:{OTHER_ACCOUNT}:User/stranger"),
        "producer_id": "connector:stranger",
        "external_id": serde_json::Value::Null,
        "evidence_type": "Provenance",
        "content_digest": "sha256:poisoned",
        "content": {"digest": "sha256:poisoned"},
        "observed_at": surrealdb::sql::Datetime::from(chrono::Utc::now()),
        "recorded_at": surrealdb::sql::Datetime::from(chrono::Utc::now()),
        "verification_status": "Verified",
        "account": OTHER_ACCOUNT,
        "subjects": [subject.to_string()],
        "record": {"seed": "poisoned"},
    });
    db.query("INSERT INTO evidence $row")
        .bind(("row", poisoned))
        .await
        .expect("insert the poisoned row")
        .check()
        .expect("insert the poisoned row check");
    db.query("INSERT INTO evidence_subject $row")
        .bind((
            "row",
            serde_json::json!({
                "account": OTHER_ACCOUNT,
                "subject": subject.to_string(),
                "evidence_id": "poisoned-ev",
            }),
        ))
        .await
        .expect("insert the poisoned pair")
        .check()
        .expect("insert the poisoned pair check");

    // A row that names the subject but cannot be read back as evidence:
    // the read drops it, so a corrupt record never comes back as
    // evidence and the capsule never affirms what it cannot support.
    let corrupt = serde_json::json!({
        "evidence_id": "corrupt-ev",
        "tenant_hrn": format!("hrn:hodei:iam:eu:{ACCOUNT}:Artifact/corrupt"),
        "producer_id": "connector:perf-lab",
        "external_id": serde_json::Value::Null,
        "evidence_type": "Provenance",
        "content_digest": "sha256:corrupt",
        "content": {"digest": "sha256:corrupt"},
        "observed_at": surrealdb::sql::Datetime::from(chrono::Utc::now()),
        "recorded_at": surrealdb::sql::Datetime::from(chrono::Utc::now()),
        "verification_status": "Verified",
        "account": ACCOUNT,
        "subjects": [subject.to_string()],
        "record": "this is not the record of an evidence",
    });
    db.query("INSERT INTO evidence $row")
        .bind(("row", corrupt))
        .await
        .expect("insert the corrupt row")
        .check()
        .expect("insert the corrupt row check");
    db.query("INSERT INTO evidence_subject $row")
        .bind((
            "row",
            serde_json::json!({
                "account": ACCOUNT,
                "subject": subject.to_string(),
                "evidence_id": "corrupt-ev",
            }),
        ))
        .await
        .expect("insert the corrupt pair")
        .check()
        .expect("insert the corrupt pair check");

    // The read through the port: exactly the three records of the
    // subject, from a table that holds the distractor, the poisoned row
    // and the corrupt row besides them.
    let read = store
        .evidence_of_subject(ACCOUNT, &subject)
        .await
        .expect("the read of the subject holds");
    let mut read_ids: Vec<&str> = read.iter().map(|record| record.id().as_str()).collect();
    read_ids.sort_unstable();
    assert_eq!(
        read_ids,
        vec!["store-ev-0", "store-ev-1", "store-ev-2"],
        "the read returns exactly the records of the subject, once each"
    );

    // The containment shape, over the same seeded tables, hands back the
    // same evidence: the fix changed the cost of the read, not its
    // answer.
    let mut response = db
        .query(
            "SELECT VALUE evidence_id FROM type::table($table) \
             WHERE account = $account AND subjects CONTAINS $subject",
        )
        .bind(("table", "evidence"))
        .bind(("account", ACCOUNT))
        .bind(("subject", subject.to_string()))
        .await
        .expect("the containment read holds")
        .check()
        .expect("the containment read check");
    let containment: Vec<String> = response.take(0).expect("containment rows");
    let mut containment = containment
        .into_iter()
        .filter(|id| id.starts_with("store-ev-"))
        .collect::<Vec<_>>();
    containment.sort();
    assert_eq!(
        containment,
        vec![
            "store-ev-0".to_string(),
            "store-ev-1".to_string(),
            "store-ev-2".to_string()
        ],
        "the containment shape reads the same evidence the port reads"
    );

    // A subject that holds no evidence reads as empty, and a read does
    // not write: the census of both tables is where it was.
    let stranger = kernel::Hrn::from_string(&subject_hrn(OTHER_ACCOUNT, 0)).expect("hrn");
    let empty = store
        .evidence_of_subject(ACCOUNT, &stranger)
        .await
        .expect("the read of a stranger subject holds");
    assert!(
        empty.is_empty(),
        "a subject with no evidence reads as nothing"
    );
    let (evidence_rows, pair_rows) = row_census(&db).await;
    assert_eq!(
        evidence_rows,
        DISTRACTOR_ROWS + 5,
        "the evidence table holds the distractor, the three records, the poisoned and the corrupt row"
    );
    assert_eq!(
        pair_rows,
        DISTRACTOR_ROWS + 6,
        "the pair table holds one entry per pair of subject and record: \
         the two-subject record contributes its two"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every record written leaves one entry per subject it speaks about,
/// and rewriting the same record rewrites the same entries: the write
/// stays idempotent, and the pair table grows by the pairs there are,
/// not by the writes there are.
#[tokio::test]
async fn the_write_leaves_one_entry_per_subject_and_stays_idempotent() {
    let dir = scratch("census");
    let db = open(&dir).await;
    declare_indexes(&db)
        .await
        .expect("the indexes are declared");
    let store = Arc::new(SurrealEvidenceStore::new(Arc::new(db.clone())));
    seed_distractor(&db, DISTRACTOR_ROWS).await;

    let subject = kernel::Hrn::from_string(&subject_hrn(ACCOUNT, 0)).expect("hrn");
    let second = kernel::Hrn::from_string(&subject_hrn(ACCOUNT, 77)).expect("hrn");

    // One record speaking about two subjects: two entries, one per
    // subject, and no entry for a subject it does not speak about.
    store
        .put(record("the-record", vec![subject.clone(), second.clone()]))
        .await
        .expect("the record exists");
    async fn pair_count(db: &Surreal<surrealdb::engine::local::Db>, subject: &str) -> usize {
        let mut response = db
            .query(
                "SELECT VALUE evidence_id FROM type::table($table) \
                 WHERE account = $account AND subject = $subject",
            )
            .bind(("table", EVIDENCE_SUBJECT_TABLE))
            .bind(("account", ACCOUNT))
            .bind(("subject", subject.to_string()))
            .await
            .expect("pair count")
            .check()
            .expect("pair count check");
        let ids: Vec<String> = response.take(0).expect("pair ids");
        ids.len()
    }
    assert_eq!(
        pair_count(&db, &subject.to_string()).await,
        1,
        "one entry for the subject the record speaks about"
    );
    assert_eq!(
        pair_count(&db, &second.to_string()).await,
        1,
        "one entry for the second subject the record speaks about"
    );
    assert_eq!(
        pair_count(&db, &subject_hrn(ACCOUNT, 90)).await,
        0,
        "no entry for a subject no record speaks about"
    );

    // The same record again: the same entries, and nothing more.
    store
        .put(record("the-record", vec![subject.clone(), second.clone()]))
        .await
        .expect("the same record written again holds");
    assert_eq!(
        pair_count(&db, &subject.to_string()).await,
        1,
        "rewriting the record rewrites its entry, it does not add one"
    );
    assert_eq!(
        pair_count(&db, &second.to_string()).await,
        1,
        "rewriting the record rewrites both entries, it does not add"
    );

    // The census: the distractor pairs are exactly where they were, and
    // the write added only the pairs of its own subjects.
    let (evidence_rows, pair_rows) = row_census(&db).await;
    assert_eq!(evidence_rows, DISTRACTOR_ROWS + 1, "one record was written");
    assert_eq!(
        pair_rows,
        DISTRACTOR_ROWS + 2,
        "the write added exactly the pairs of its own subjects"
    );

    // The entries the write left are the entries the read walks: the
    // record is found from both of its subjects.
    let from_subject = store
        .evidence_of_subject(ACCOUNT, &subject)
        .await
        .expect("the read of the subject holds");
    assert_eq!(
        from_subject.len(),
        1,
        "the record is found from one subject"
    );
    let from_second = store
        .evidence_of_subject(ACCOUNT, &second)
        .await
        .expect("the read of the second subject holds");
    assert_eq!(
        from_second.len(),
        1,
        "the same record is found from the other subject too"
    );
    assert_eq!(
        from_subject[0].id(),
        from_second[0].id(),
        "both subjects name the same record"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
