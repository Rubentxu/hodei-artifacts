//! Projection rebuild and semantic reconciliation of the cutover (HSP-211).
//!
//! On top of HSP-210's import and its portable fixture
//! `tests/fixtures/hsp210-authorities` (31 rows, 13 tables, 16 batches), this
//! IT exercises the `src/recovery/reconcile.rs` module:
//!
//! - AC1: source (package) and destination (clean import) are equivalent per account,
//!   current grants, tags, paths, digests and cursors, and the gate delivers
//!   the cutover token. Any divergence -- missing, content,
//!   count compensation, CAS without the blob, a lying cursor -- aborts
//!   with `Blocked` naming the culprit account or row: DIVERGENCE PREVENTS
//!   CUTOVER, it is not a warning.
//! - AC2: the rebuild is idempotent -- preparing the same state twice
//!   preserves the semantic version and the fingerprint, and importing the destination
//!   twice (kill/resume of the 210 contract) leaves the projection intact.
//!   Missing/corrupt never escalates to Verified: corrupt states abort
//!   the rebuild itself, and the `CutoverToken` type is not fabricable.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hodei_artifacts_api::recovery::import::{
    ImportOptions, MemorySink, Row, SetReconciler, read_batch,
};
use hodei_artifacts_api::recovery::read_import_manifest;
use hodei_artifacts_api::recovery::reconcile::{
    CutoverDecision, Divergence, ReconcileError, package_rows, prepare, prepare_from_sink,
};
use serde_json::Value;

const PACKAGE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/hsp210-authorities"
);

/// Keys of the fixture rows that carry a content digest;
/// the same ones declared by the import's `DIGEST_FIELDS`.
const DIGEST_KEYS: [(&str, &str); 5] = [
    ("catalog_asset", "sha256"),
    ("catalog_reference", "sha256"),
    ("provenance_receipt", "digest"),
    ("trust_grant", "evidence"),
    ("catalog_audit_outbox", "payload_digest"),
];

fn package_dir() -> PathBuf {
    Path::new(PACKAGE).to_path_buf()
}

fn options(batch_rows: usize) -> ImportOptions {
    ImportOptions {
        batch_rows,
        ..ImportOptions::default()
    }
}

/// The test CAS: knows exactly the digests the fixture
/// references, read from the package itself, except those that are withdrawn.
fn fixture_cas(withdrawn: &[&str]) -> SetReconciler {
    let manifest = read_import_manifest(&package_dir()).unwrap();
    let mut digests: BTreeSet<String> = BTreeSet::new();
    for table in &manifest.tables {
        for batch in &table.batches {
            let rows: Vec<Row> = read_batch(&package_dir(), batch).unwrap();
            for row in rows {
                for (name, field) in DIGEST_KEYS {
                    if table.table == name
                        && let Some(Value::String(digest)) = row.get(field)
                    {
                        digests.insert(digest.clone());
                    }
                }
            }
        }
    }
    for digest in withdrawn {
        digests.remove(*digest);
    }
    SetReconciler::new(digests)
}

/// The oracle: a destination with the fixture imported once, clean.
async fn imported_destination() -> Arc<MemorySink> {
    let dir = tempfile::tempdir().unwrap();
    let sink: Arc<MemorySink> = Arc::new(MemorySink::new());
    let report = hodei_artifacts_api::recovery::import_export(
        &package_dir(),
        sink.clone(),
        Some(Arc::new(fixture_cas(&[]))),
        &options(4),
        &dir.path().join(".import-progress.json"),
    )
    .await
    .expect("the fixture imports cleanly");
    assert_eq!(report.created, 31);
    assert!(report.finished);
    // The import's progress cannot survive the run.
    assert!(!dir.path().join(".import-progress.json").exists());
    sink
}

fn origin_state() -> hodei_artifacts_api::recovery::reconcile::ReconcileSubject {
    prepare(package_rows(&package_dir()).unwrap()).expect("the fixture is projectable")
}

/// AC1, happy path: package ↔ imported destination are equivalent in all
/// projections and the gate issues the token with the verified fingerprints.
#[tokio::test]
async fn the_gate_verifies_a_faithful_import() {
    let origin = origin_state();
    let sink = imported_destination().await;
    let destination = prepare_from_sink(sink.as_ref()).await.unwrap();

    assert_eq!(origin.fingerprint, destination.fingerprint);
    assert_eq!(origin.version(), destination.version());

    let decision = hodei_artifacts_api::recovery::reconcile::gate_cutover(
        &origin,
        &destination,
        &fixture_cas(&[]),
    )
    .await
    .unwrap();
    let CutoverDecision::Verified(token) = &decision else {
        panic!("a faithful destination must clear the gate, got {decision:?}");
    };
    assert_eq!(token.origin_fingerprint(), origin.fingerprint.as_str());
    assert_eq!(
        token.destination_fingerprint(),
        destination.fingerprint.as_str()
    );
    assert_eq!(token.version(), origin.version().as_str());
    eprintln!(
        "HSP211 cutover verified, authorities = {}",
        token.origin_fingerprint()
    );
}

/// AC1 fail-closed: a missing destination row produces Blocked
/// naming the key, and never a token.
#[tokio::test]
async fn missing_rows_block_the_cutover_naming_the_culprit() {
    let origin = origin_state();
    let sink = imported_destination().await;
    let mut rows = sink.snapshot().await;
    rows.remove("catalog_version:globex|main|gadget|0.1.0");
    let destination = prepare(rows).unwrap();

    let decision = hodei_artifacts_api::recovery::reconcile::gate_cutover(
        &origin,
        &destination,
        &fixture_cas(&[]),
    )
    .await
    .unwrap();
    let CutoverDecision::Blocked(divergences) = &decision else {
        panic!("a missing row must block, got {decision:?}");
    };
    assert!(token_of(&decision).is_none());
    assert!(
        divergences
            .iter()
            .any(|d| d.to_string().contains("account 'globex'")),
        "the account that lost a row must be named: {divergences:?}"
    );
}

/// AC1 against isolated counts: moving a row from one account to another leaves the
/// total count identical (31) but the projections diverge -- the gate
/// sees it and aborts. The global count does not buy a green.
#[tokio::test]
async fn compensating_counts_are_a_divergence_not_a_pass() {
    let origin = origin_state();
    let sink = imported_destination().await;
    let mut rows = sink.snapshot().await;
    // latest leaves acme and appears, with other content, in globex:
    // same total of rows, different accounts.
    rows.remove("catalog_tag:acme|main|latest");
    let mut moved = Row::new();
    moved.insert(
        "id".into(),
        Value::String("catalog_tag:globex|main|latest".into()),
    );
    moved.insert("account".into(), Value::String("globex".into()));
    moved.insert("repository".into(), Value::String("main".into()));
    moved.insert("tag_name".into(), Value::String("latest".into()));
    moved.insert("target_version".into(), Value::String("0.1.0".into()));
    rows.insert("catalog_tag:globex|main|latest".into(), moved);
    assert_eq!(rows.len(), 31, "the census is deliberately unchanged");

    let destination = prepare(rows).unwrap();
    let decision = hodei_artifacts_api::recovery::reconcile::gate_cutover(
        &origin,
        &destination,
        &fixture_cas(&[]),
    )
    .await
    .unwrap();
    match &decision {
        CutoverDecision::Blocked(divergences) => {
            assert!(
                divergences
                    .iter()
                    .any(|d| d.to_string().contains("account 'acme'"))
                    && divergences
                        .iter()
                        .any(|d| d.to_string().contains("account 'globex'")),
                "both displaced accounts must be named: {divergences:?}"
            );
        }
        CutoverDecision::Verified(_) => panic!("equal counts are not equivalence"),
    }
}

/// AC1: a digest that the CAS does not have is missing content, not silence.
/// The gate names the digest and the row that references it.
#[tokio::test]
async fn a_digest_the_cas_lacks_blocks_the_cutover() {
    let origin = origin_state();
    let sink = imported_destination().await;
    let destination = prepare_from_sink(sink.as_ref()).await.unwrap();
    let missing = "sha256:a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
    let decision = hodei_artifacts_api::recovery::reconcile::gate_cutover(
        &origin,
        &destination,
        &fixture_cas(&[missing]),
    )
    .await
    .unwrap();
    let CutoverDecision::Blocked(divergences) = &decision else {
        panic!("a blob the CAS lacks must block, got {decision:?}");
    };
    assert!(
        divergences.iter().any(|d| {
            matches!(d, Divergence::MissingDigest { digest, referenced_by }
                if digest == missing && referenced_by.starts_with("catalog_asset:"))
        }),
        "the missing digest must name its referent: {divergences:?}"
    );
}

/// AC2: the rebuild is idempotent. Preparing the same state twice gives the
/// same version and the same fingerprint; importing the destination a second
/// time (all skipped) moves neither the projection nor the gate's result.
#[tokio::test]
async fn rebuilding_twice_preserves_version_and_results() {
    let origin = origin_state();
    let again = prepare(package_rows(&package_dir()).unwrap()).unwrap();
    assert_eq!(origin.version(), again.version());
    assert_eq!(origin.fingerprint, again.fingerprint);

    let sink = imported_destination().await;
    let first = prepare_from_sink(sink.as_ref()).await.unwrap();

    let dir = tempfile::tempdir().unwrap();
    let report = hodei_artifacts_api::recovery::import_export(
        &package_dir(),
        sink.clone(),
        Some(Arc::new(fixture_cas(&[]))),
        &options(4),
        &dir.path().join(".import-progress.json"),
    )
    .await
    .expect("re-importing the same package is idempotent");
    assert_eq!(report.created, 0);
    assert_eq!(report.skipped, 31);

    let second = prepare_from_sink(sink.as_ref()).await.unwrap();
    assert_eq!(first.version(), second.version());
    assert_eq!(first.fingerprint, second.fingerprint);

    let decision =
        hodei_artifacts_api::recovery::reconcile::gate_cutover(&origin, &second, &fixture_cas(&[]))
            .await
            .unwrap();
    assert!(
        matches!(decision, CutoverDecision::Verified(_)),
        "the gate result survived the rebuild: {decision:?}"
    );
}

/// AC2 (vocabulary): a required row without its key field is corruption
/// and aborts the rebuild naming the row; there is no path from there to
/// `Verified`.
#[tokio::test]
async fn a_corrupt_row_fails_the_rebuild_not_the_gate() {
    let mut rows = package_rows(&package_dir()).unwrap();
    let version = rows
        .get_mut("catalog_version:acme|main|widget|1.0.0")
        .expect("the fixture carries it");
    version.remove("account");
    let error = prepare(rows).unwrap_err();
    assert!(
        matches!(&error, ReconcileError::Corrupt { identity, detail }
            if identity == "catalog_version:acme|main|widget|1.0.0"
                && detail.contains("'account'")),
        "the corrupt row must be named: {error}"
    );
}

/// AC2 (vocabulary): a head that lies about its ledger is corrupt by
/// definition; the rebuild aborts with the culprit stream before the
/// gate can say anything.
#[tokio::test]
async fn a_lying_ledger_head_is_corruption() {
    let mut rows = package_rows(&package_dir()).unwrap();
    let head = rows
        .get_mut("event_ledger_head:stream_a")
        .expect("the fixture carries it");
    head.insert("last_position".into(), Value::Number(99.into()));
    let error = prepare(rows).unwrap_err();
    assert!(
        matches!(&error, ReconcileError::CursorMismatch { stream, head, events }
            if stream == "stream_a" && *head == 99 && *events == 2),
        "the lying head must name its stream: {error}"
    );
}

/// Paths are compared by their logical key: an asset moved between
/// repositories changes the account's bundle and the paths aggregate, and the
/// gate sees it even if no table count moves.
#[tokio::test]
async fn paths_are_their_own_aggregate() {
    let origin = origin_state();
    assert!(
        origin
            .projection
            .paths
            .contains_key("acme|main|widget-1.0.0.bin"),
        "the fixture's assets are projected by path"
    );
    assert!(
        origin.projection.paths.contains_key("acme|main|dep.txt"),
        "references are projected by path too"
    );
    let sink = imported_destination().await;
    let mut rows = sink.snapshot().await;
    let asset = rows
        .get_mut("catalog_asset:acme|main|widget-1.0.0.bin")
        .expect("imported");
    asset.insert("repository".into(), Value::String("mirror".into()));
    let destination = prepare(rows).unwrap();
    let decision = hodei_artifacts_api::recovery::reconcile::gate_cutover(
        &origin,
        &destination,
        &fixture_cas(&[]),
    )
    .await
    .unwrap();
    let CutoverDecision::Blocked(divergences) = &decision else {
        panic!("a relocated artifact must block, got {decision:?}");
    };
    assert!(
        divergences.iter().any(|d| {
            matches!(d, Divergence::Missing { what } if what.contains("path 'acme|main|widget-1.0.0.bin'"))
        }),
        "the orphaned path must be named: {divergences:?}"
    );
}

fn token_of(
    decision: &CutoverDecision,
) -> Option<&hodei_artifacts_api::recovery::reconcile::CutoverToken> {
    decision.token()
}

/// Sink contract guarantee: the destination's projection uses the same
/// canonical identities as the import; if the summary lied with a row
/// that the sink does not return, prepare_from_sink aborts as corrupt.
#[tokio::test]
async fn prepare_from_sink_walks_the_same_identities_as_import() {
    let sink = imported_destination().await;
    let subject = prepare_from_sink(sink.as_ref()).await.unwrap();
    assert_eq!(subject.rows.len(), 31);
    let table_counts: BTreeMap<&str, usize> =
        subject.rows.keys().fold(BTreeMap::new(), |mut acc, key| {
            *acc.entry(key.split(':').next().unwrap()).or_default() += 1;
            acc
        });
    assert_eq!(table_counts["event_ledger"], 4);
    assert_eq!(table_counts["trust_grant"], 2);
}
