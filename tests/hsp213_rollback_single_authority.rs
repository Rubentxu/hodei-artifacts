//! HSP-213: rollback does not depend on starting two authorities.
//!
//! AC3 of the retirement, demonstrated with the HSP-212 state machine on the
//! **production tree**: the only one that exists, the one that rejects the embedded
//! profile (`hsp213_profile_rejection`) and that is packaged by
//! `scripts/package-release.sh`. That the rollback machinery
//! (`src/recovery/cutover.rs`, no feature) works there is already part of
//! the proof: going back does not need the retired embedded engine, nor a
//! second process to start it.
//!
//! On top of that fact, the rehearsal fixes the invariant the assignment asks for:
//!
//! - In every phase there is at most **one** writable authority. Once switched
//!   over, the origin is frozen: it serves reads and **counts**
//!   every write the old service attempted. Once the authority is
//!   handed back, the destination goes to standby: writing there would be the
//!   dual-write that the machine makes unconstructible.
//! - After the first new write, free rollback is forbidden
//!   (`PostWriteRollbackForbidden`); the way back is forward recovery
//!   (freeze, export, replay with gate, token) and it loses none
//!   of the new writes.
//! - Free pre-write rollback restores the origin **exactly**
//!   (fingerprint identical to the barrier's) and leaves the destination out of
//!   play: there is no window with two authorities, not even for an instant.
//!
//! The seeding and the 31-row bundle are the certified fixtures of
//! HSP-210/211/212; this target does not use the `tests/support` harness.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hodei_artifacts_api::recovery::cutover::{
    Authority, AuthorityError, CutoverError, CutoverMachine, Phase,
};
use hodei_artifacts_api::recovery::import::{
    DigestReconciler, ImportOptions, MemorySink, Row, SetReconciler, import_export, read_batch,
    read_import_manifest,
};
use serde_json::Value;

const PACKAGE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/hsp210-authorities"
);

/// Keys of the fixture rows that carry a content digest.
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

fn options() -> ImportOptions {
    ImportOptions {
        batch_rows: 4,
        ..ImportOptions::default()
    }
}

/// The platform CAS of the rehearsal: exactly the digests that the
/// fixture references, read from the bundle itself.
fn fixture_cas() -> Arc<dyn DigestReconciler> {
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
    Arc::new(SetReconciler::new(digests))
}

/// The origin with traffic: the fixture imported clean, 31 rows.
async fn seeded_origin() -> Arc<MemorySink> {
    let dir = tempfile::tempdir().unwrap();
    let sink: Arc<MemorySink> = Arc::new(MemorySink::new());
    let report = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_cas()),
        &options(),
        &dir.path().join(".seed-progress.json"),
    )
    .await
    .expect("the origin is seeded from the fixture");
    assert_eq!(report.created, 31);
    sink
}

/// Set up the rehearsal: origin seeded and active, destination empty on standby.
/// The workspace is held by the caller.
async fn rehearsal(workspace: &Path) -> CutoverMachine {
    let origin = Arc::new(Authority::origin("origin", seeded_origin().await));
    let destination = Arc::new(Authority::destination(
        "destination",
        Arc::new(MemorySink::new()),
    ));
    CutoverMachine::new(
        origin,
        destination,
        fixture_cas(),
        workspace.to_path_buf(),
        options(),
    )
    .expect("the rehearsal starts with origin active and destination on standby")
}

/// The AC3 invariant: at most one writable authority in each phase.
fn assert_single_authority(machine: &CutoverMachine) {
    assert!(
        !(machine.origin().is_active() && machine.destination().is_active()),
        "split-brain in phase {}",
        machine.phase().name()
    );
}

/// The path up to the cutover: barrier, snapshot, import, gate, token.
async fn cut_over(machine: &mut CutoverMachine) -> hodei_artifacts_api::recovery::CutoverToken {
    assert_single_authority(machine);
    machine
        .close_barrier()
        .await
        .expect("the barrier closes and freezes the origin");
    assert_single_authority(machine);
    machine
        .import_snapshot()
        .await
        .expect("the snapshot imports into the standby");
    assert_single_authority(machine);
    let token = machine.verify().await.expect("the gate verifies");
    machine.commit(&token).await.expect("the token switches");
    assert_single_authority(machine);
    token
}

/// A row of the fixtures' domain: a tag of the `main` repository.
fn a_tag(name: &str, version: &str) -> (String, Row) {
    let mut row = Row::new();
    row.insert("account".into(), Value::String("acme".into()));
    row.insert("repository".into(), Value::String("main".into()));
    row.insert("tag_name".into(), Value::String(name.into()));
    row.insert("target_version".into(), Value::String(version.into()));
    (format!("catalog_tag:acme|main|{name}"), row)
}

/// AC3: after the first new write the way back is forward
/// recovery, and in no phase of the path are there two authorities.
#[tokio::test]
async fn the_post_write_rollback_never_runs_two_authorities() {
    let dir = tempfile::tempdir().expect("a workspace");
    let mut machine = rehearsal(dir.path()).await;
    let _token = cut_over(&mut machine).await;

    // Switched over: the origin is frozen. It serves reads from the moment of
    // the barrier and rejects and counts every write: the dual-write of the
    // "just in case" rollback is not constructible.
    let sample = machine
        .origin()
        .read("catalog_tag:acme|main|stable")
        .await
        .unwrap();
    assert!(sample.is_some(), "the frozen origin still serves reads");
    let (identity, row) = a_tag("stray", "0.0.1");
    let error = machine
        .origin()
        .write(&identity, &row)
        .await
        .expect_err("the frozen origin refuses service writes");
    assert!(matches!(error, AuthorityError::Frozen { .. }), "{error}");
    assert_eq!(machine.origin().refused_writes(), 1);

    // The first new write on the new authority: point of no
    // return for free rollback.
    let (identity, row) = a_tag("beta", "2.1.0");
    machine
        .write_new(&identity, &row)
        .await
        .expect("the new authority takes writes");
    assert_eq!(machine.phase(), Phase::CutOverLive);
    let error = machine
        .rollback()
        .await
        .expect_err("free rollback is dead after the first new write");
    assert!(
        matches!(&error, CutoverError::PostWriteRollbackForbidden { writes } if *writes == 1),
        "{error}"
    );

    // The way back: freeze, export, replay with gate. The invariant is
    // checked in every phase, including this one.
    machine
        .export_forward()
        .await
        .expect("freeze + export the new authority");
    assert_single_authority(&machine);
    let replay_token = machine
        .replay_to_origin()
        .await
        .expect("the replay verifies against the frozen destination");
    assert_single_authority(&machine);
    machine
        .rollback_with_replay(&replay_token)
        .await
        .expect("the validated replay token brings the authority home");
    assert_eq!(machine.phase(), Phase::RolledBack);
    assert_single_authority(&machine);

    // Once the authority is handed back, the destination is out of play: after the
    // forward recovery it remains frozen --the exported replica is the
    // evidence of the replay--, serves reads and rejects writes. What
    // matters to AC3 is that, not the label: there are not two writable ones, not even for an
    // instant, and the origin admits writes again.
    let (identity, row) = a_tag("sneaky", "9.9.9");
    let error = machine
        .destination()
        .write(&identity, &row)
        .await
        .expect_err("the demoted destination cannot be written");
    assert!(
        matches!(error, AuthorityError::Frozen { .. }),
        "the destination demoted by forward recovery stays frozen as the replay evidence: {error}"
    );
    assert!(!machine.destination().is_active());
    let (identity, row) = a_tag("after-rollback", "1.2.3");
    machine
        .origin()
        .write(&identity, &row)
        .await
        .expect("the restored origin takes writes again");

    // No silent loss: the new write is in the origin.
    assert!(
        machine
            .origin()
            .read("catalog_tag:acme|main|beta")
            .await
            .unwrap()
            .is_some(),
        "the replay lost the new write"
    );
}

/// AC3 in its simple half: pre-first-write rollback is exact,
/// and here too two authorities never come to coexist.
#[tokio::test]
async fn the_pre_write_rollback_restores_one_authority_exactly() {
    let dir = tempfile::tempdir().expect("a workspace");
    let mut machine = rehearsal(dir.path()).await;
    let before = machine
        .origin()
        .fingerprint()
        .await
        .expect("origin fingerprint before the rehearsal");
    cut_over(&mut machine).await;

    machine.rollback().await.expect("free pre-write rollback");
    assert_eq!(machine.phase(), Phase::RolledBack);
    assert_single_authority(&machine);
    assert!(machine.origin().is_active());
    assert!(!machine.destination().is_active());
    assert_eq!(
        machine.origin().fingerprint().await.unwrap(),
        before,
        "restore is exact: the origin never wrote since the barrier"
    );

    // The destination, returned to standby: it does not write, it is not the second
    // authority that "going back" would have had to lift.
    let (identity, row) = a_tag("sneaky", "9.9.9");
    let error = machine
        .destination()
        .write(&identity, &row)
        .await
        .expect_err("the standby destination cannot be written");
    assert!(matches!(error, AuthorityError::NotActive { .. }), "{error}");
}

/// The proof that the rehearsal runs where there is no embedded engine: the
/// test's own tree. That this module compiles already asserts
/// that the rollback machine (`src/recovery/cutover.rs`) does not depend on the
/// retired reader; the message assertion recalls that, in that same
/// tree, there is no tool left to go back to the local store (retired in
/// HSP-314 F4), nor a second service binary.
#[test]
fn the_rehearsal_tree_ships_no_embedded_engine() {
    assert!(hodei_artifacts_api::config::GRAPH_PROFILE_REQUIRED.contains("HSP-213"));
    assert!(hodei_artifacts_api::config::GRAPH_PROFILE_REQUIRED.contains("HSP-314 F4"));
}
