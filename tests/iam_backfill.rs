//! Legacy IAM policy backfill: dry-run, copy, revoke, conflicts and rollback.
//!
//! The migration touches live policy data, so the suite covers the properties
//! that make it safe to run: the copy and the revocation travel in one
//! transaction, ownership that cannot be established is reported instead of
//! guessed, and a scoped row holding a different policy is never overwritten.

use std::collections::HashMap;
use std::process::Command;

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
    #[serde(default)]
    #[allow(dead_code)]
    account: Option<String>,
}

async fn exists(db: &Surreal<Db>, id: &str) -> bool {
    let row: Option<Row> = db.select(("policy", id)).await.unwrap();
    row.is_some()
}

async fn row(db: &Surreal<Db>, id: &str) -> Option<Row> {
    db.select(("policy", id)).await.unwrap()
}

async fn seed(db: &Surreal<Db>, id: &str, content: &str, principals: &[&str]) {
    let _: Option<Row> = db
        .create(("policy", id))
        .content(json!({
            "content": content,
            "attached_principals": principals,
        }))
        .await
        .unwrap();
}

/// The plan entry of one legacy row.
///
/// The plan also lists the rows that already carry a scoped id, so an entry
/// has to be looked up by its legacy id instead of by position.
fn entry<'a>(
    report: &'a hodei_artifacts_api::iam_backfill::BackfillReport,
    legacy_id: &str,
) -> &'a hodei_artifacts_api::iam_backfill::BackfillEntry {
    report
        .entries
        .iter()
        .find(|entry| entry.legacy_id == legacy_id)
        .unwrap_or_else(|| {
            panic!(
                "the plan must contain `{legacy_id}`, got {:?}",
                report.entries
            )
        })
}

const ALICE: &str = "hrn:hodei:iam::acme:User/alice";

#[tokio::test]
async fn dry_run_then_apply_copies_and_revokes_the_legacy_row() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    seed(
        &db,
        "legacy-a",
        "permit(principal, action, resource);",
        &[ALICE],
    )
    .await;

    let mapping = HashMap::new();
    let dry = run(&db, &mapping, false).await.unwrap();
    assert_eq!(dry.migrated, 0);
    assert_eq!(dry.entries.len(), 1);
    assert_eq!(dry.entries[0].status, BackfillStatus::Ready);
    assert_eq!(dry.entries[0].account.as_deref(), Some("acme"));
    assert_eq!(dry.entries[0].target_id.as_deref(), Some("acme~legacy-a"));
    assert!(exists(&db, "legacy-a").await);
    assert!(!exists(&db, "acme~legacy-a").await);

    let report = run(&db, &mapping, true).await.unwrap();
    assert_eq!(report.migrated, 1);
    assert_eq!(report.ambiguous, 0);
    assert_eq!(report.conflicts, 0);
    assert!(exists(&db, "acme~legacy-a").await);
    assert!(!exists(&db, "legacy-a").await);

    // The copy preserves the policy content, its attachments and the scope.
    let copied = row(&db, "acme~legacy-a").await.expect("scoped copy");
    assert_eq!(copied.content, "permit(principal, action, resource);");
    assert_eq!(copied.attached_principals, vec![ALICE.to_string()]);
    assert_eq!(copied.account.as_deref(), Some("acme"));

    // Idempotent: a second run has nothing left to migrate.
    let again = run(&db, &mapping, true).await.unwrap();
    assert_eq!(again.migrated, 0);
    assert!(exists(&db, "acme~legacy-a").await);
    assert!(!exists(&db, "legacy-a").await);

    // The evaluator selects every row attached to a principal, without
    // filtering by scope, so the policy has to be effective exactly once
    // after the backfill: the scoped copy, and no leftover legacy row.
    let mut response = db
        .query("SELECT * FROM policy WHERE $principal IN attached_principals")
        .bind(("principal", ALICE))
        .await
        .unwrap();
    assert!(response.take_errors().is_empty());
    let effective: Vec<Row> = response.take(0).unwrap();
    assert_eq!(
        effective.len(),
        1,
        "the policy must be effective exactly once"
    );
}

#[tokio::test]
async fn ambiguous_attachments_are_not_migrated() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    seed(
        &db,
        "legacy-ambiguous",
        "permit(principal, action, resource);",
        &[ALICE, "hrn:hodei:iam::other:User/bob"],
    )
    .await;

    let report = run(&db, &HashMap::new(), true).await.unwrap();
    assert_eq!(report.migrated, 0);
    assert_eq!(report.ambiguous, 1);
    assert!(exists(&db, "legacy-ambiguous").await);
    assert!(!exists(&db, "acme~legacy-ambiguous").await);
}

#[tokio::test]
async fn an_unparsable_attachment_makes_the_row_ambiguous() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    // One valid principal and one that is not an HRN. The invalid one must not
    // be dropped in silence, because then the row would look unambiguous.
    seed(
        &db,
        "legacy-invalid-hrn",
        "permit(principal, action, resource);",
        &[ALICE, "not-an-hrn"],
    )
    .await;

    let report = run(&db, &HashMap::new(), true).await.unwrap();
    assert_eq!(report.migrated, 0);
    assert_eq!(report.ambiguous, 1);
    let reason = match &entry(&report, "legacy-invalid-hrn").status {
        BackfillStatus::Ambiguous(reason) => reason.clone(),
        other => panic!("expected an ambiguous row, got {other:?}"),
    };
    assert!(
        reason.contains("not-an-hrn"),
        "the reason must name the offending principal: {reason}"
    );
    assert!(exists(&db, "legacy-invalid-hrn").await);
    assert!(!exists(&db, "acme~legacy-invalid-hrn").await);
}

#[tokio::test]
async fn a_row_without_principals_or_mapping_is_ambiguous() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    seed(
        &db,
        "legacy-no-owner",
        "permit(principal, action, resource);",
        &[],
    )
    .await;

    let report = run(&db, &HashMap::new(), true).await.unwrap();
    assert_eq!(report.migrated, 0);
    assert_eq!(report.ambiguous, 1);
    assert!(exists(&db, "legacy-no-owner").await);
    assert!(!exists(&db, "acme~legacy-no-owner").await);
}

#[tokio::test]
async fn an_explicit_mapping_resolves_a_row_without_principals() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    seed(
        &db,
        "legacy-mapped",
        "permit(principal, action, resource);",
        &[],
    )
    .await;

    let mapping = HashMap::from([("legacy-mapped".to_string(), "acme".to_string())]);
    let report = run(&db, &mapping, true).await.unwrap();
    assert_eq!(report.migrated, 1);
    assert_eq!(report.ambiguous, 0);
    assert!(exists(&db, "acme~legacy-mapped").await);
    assert!(!exists(&db, "legacy-mapped").await);
}

#[tokio::test]
async fn a_scoped_row_holding_a_different_policy_is_a_conflict() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    seed(
        &db,
        "legacy-x",
        "permit(principal, action, resource);",
        &[ALICE],
    )
    .await;
    // A scoped row already exists for the same target with another policy.
    let _: Option<Row> = db
        .create(("policy", "acme~legacy-x"))
        .content(json!({
            "content": "permit(principal, action == Other::Action::\"X\", resource);",
            "attached_principals": [ALICE],
            "account": "acme",
        }))
        .await
        .unwrap();

    let report = run(&db, &HashMap::new(), true).await.unwrap();
    assert_eq!(report.migrated, 0);
    assert_eq!(report.conflicts, 1);
    assert_eq!(report.ambiguous, 0);
    let reason = match &entry(&report, "legacy-x").status {
        BackfillStatus::Conflict(reason) => reason.clone(),
        other => panic!("expected a conflict, got {other:?}"),
    };
    assert!(
        reason.contains("acme~legacy-x"),
        "the reason must name the conflicting row: {reason}"
    );

    // Neither row is touched: the existing policy keeps its content and the
    // legacy row is not revoked.
    let scoped = row(&db, "acme~legacy-x").await.expect("scoped row");
    assert!(scoped.content.contains("Other::Action"));
    assert!(exists(&db, "legacy-x").await);
}

#[tokio::test]
async fn a_dry_run_reports_conflicts_without_writing() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    seed(
        &db,
        "legacy-x",
        "permit(principal, action, resource);",
        &[ALICE],
    )
    .await;
    let _: Option<Row> = db
        .create(("policy", "acme~legacy-x"))
        .content(json!({
            "content": "permit(principal, action == Other::Action::\"X\", resource);",
            "attached_principals": [ALICE],
            "account": "acme",
        }))
        .await
        .unwrap();

    let report = run(&db, &HashMap::new(), false).await.unwrap();
    assert_eq!(report.conflicts, 1);
    assert_eq!(report.migrated, 0);
    // The dry run wrote nothing: the legacy row is still there and the scoped
    // row still holds its own policy.
    assert!(exists(&db, "legacy-x").await);
    let scoped = row(&db, "acme~legacy-x").await.expect("scoped row");
    assert!(scoped.content.contains("Other::Action"));
}

#[tokio::test]
async fn the_legacy_row_is_revoked_when_the_scoped_copy_already_exists() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    seed(
        &db,
        "legacy-dup",
        "permit(principal, action, resource);",
        &[ALICE],
    )
    .await;
    // A previous run already copied the policy but could not revoke the row.
    let _: Option<Row> = db
        .create(("policy", "acme~legacy-dup"))
        .content(json!({
            "content": "permit(principal, action, resource);",
            "attached_principals": [ALICE],
            "account": "acme",
        }))
        .await
        .unwrap();

    let report = run(&db, &HashMap::new(), true).await.unwrap();
    assert_eq!(report.migrated, 0);
    assert_eq!(report.revoked, 1);
    assert_eq!(report.conflicts, 0);
    assert!(exists(&db, "acme~legacy-dup").await);
    assert!(!exists(&db, "legacy-dup").await);
}

#[tokio::test]
async fn a_failed_write_rolls_the_whole_batch_back() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    seed(
        &db,
        "good",
        "permit(principal, action, resource);",
        &[ALICE],
    )
    .await;
    seed(&db, "bad", "poison", &[ALICE]).await;

    // A database constraint that rejects one of the copies. It stands in for
    // any write the store refuses, and it fails in the middle of the batch.
    let mut response = db
        .query("DEFINE FIELD content ON policy TYPE string ASSERT $value != 'poison';")
        .await
        .unwrap();
    assert!(
        response.take_errors().is_empty(),
        "the constraint must be defined before the batch runs"
    );

    let report = run(&db, &HashMap::new(), true).await;
    let error = report.expect_err("the failing batch must surface an error");
    assert!(
        error.contains("rolled back"),
        "the error must state that the transaction was rolled back: {error}"
    );

    // The copy that preceded the failure is rolled back with it, and neither
    // legacy row is revoked: no policy is left effective without a scope and
    // no pair of rows is left alive.
    assert!(!exists(&db, "acme~good").await);
    assert!(!exists(&db, "acme~bad").await);
    assert!(exists(&db, "good").await);
    assert!(exists(&db, "bad").await);
}

#[tokio::test]
async fn a_failed_batch_can_be_retried_once_the_cause_is_removed() {
    let tmp = tempfile::tempdir().unwrap();
    let db = open(&tmp.path().join("db")).await;
    seed(
        &db,
        "good",
        "permit(principal, action, resource);",
        &[ALICE],
    )
    .await;
    seed(&db, "bad", "poison", &[ALICE]).await;

    let mut response = db
        .query("DEFINE FIELD content ON policy TYPE string ASSERT $value != 'poison';")
        .await
        .unwrap();
    assert!(response.take_errors().is_empty());
    assert!(run(&db, &HashMap::new(), true).await.is_err());
    assert!(exists(&db, "good").await);

    // The operator repairs the offending policy and runs the migration again.
    // Only the content is repaired: the attachments still name the owner.
    let mut repair = db
        .query("UPDATE policy:bad SET content = 'permit(principal, action, resource);';")
        .await
        .unwrap();
    assert!(repair.take_errors().is_empty());
    let mut remove = db.query("REMOVE FIELD content ON policy;").await.unwrap();
    assert!(remove.take_errors().is_empty());

    let report = run(&db, &HashMap::new(), true).await.unwrap();
    assert_eq!(report.migrated, 2);
    assert!(exists(&db, "acme~good").await);
    assert!(exists(&db, "acme~bad").await);
    assert!(!exists(&db, "good").await);
    assert!(!exists(&db, "bad").await);
}

#[test]
fn the_operator_command_runs_against_a_database_and_reports_the_plan() {
    let tmp = tempfile::tempdir().unwrap();
    let db_path = tmp.path().join("db");
    let mapping_path = tmp.path().join("mapping.json");
    std::fs::write(&mapping_path, "{}").unwrap();

    // Seed a legacy row, then release the store so the command opens it alone.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let db = open(&db_path).await;
        seed(
            &db,
            "legacy-cli",
            "permit(principal, action, resource);",
            &[ALICE],
        )
        .await;
    });

    let output = Command::new(env!("CARGO_BIN_EXE_backfill-iam-scope"))
        .arg("--db")
        .arg(&db_path)
        .arg("--mapping")
        .arg(&mapping_path)
        .output()
        .expect("the operator command must be built and runnable");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "the dry run must succeed: {stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("legacy-cli"),
        "the plan must name the legacy row: {stdout}"
    );
    assert!(
        stdout.contains("acme~legacy-cli"),
        "the plan must name the proposed account scope: {stdout}"
    );
    assert!(
        stdout.contains("apply=false"),
        "the plan must state that nothing was written: {stdout}"
    );
}
