//! Cutover and rollback rehearsal of a single authority (HSP-212, HGS-29).
//!
//! On the certified fixtures and seams of HSP-210 (idempotent import)
//! and HSP-211 (semantic gate with `CutoverToken`), this IT
//! exercises the state machine of `src/recovery/cutover.rs` with the real
//! package of 31 rows / 13 tables and demonstrates behavior, not mocks:
//!
//! - AC1: full rehearsal with real traffic -- barrier, snapshot, import,
//!   gate and switch. In the switched phase the origin is read-only
//!   (serves reads, rejects and **counts** writes) and the destination is the
//!   exclusive authority. In each phase there is at most one active
//!   authority: split-brain is not constructible. A destination dirtied from outside
//!   blocks the gate with a named culprit and makes the switch illegal.
//! - AC2: pre-first-write rollback restores the origin exactly
//!   (and refuses if the destination drifted); after the first new write
//!   free rollback is forbidden and only forward recovery
//!   (freeze → export → replay → gate → token) returns the authority
//!   without silent loss. A kill midway through the import resumes to the
//!   fingerprint identical to the clean run (CrashSink 210 pattern).
//! - AC3: the cutover window and the RPO/RTO are measured in the run and
//!   published in `target/uat/hsp212/evidencia.md` as observed
//!   values, never as a promise.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use hodei_artifacts_api::recovery::cutover::{
    Authority, AuthorityError, CutoverError, CutoverMachine, Phase,
};
use hodei_artifacts_api::recovery::import::{
    DigestReconciler, ImportFailure, ImportOptions, MemorySink, Row, RowSink, SetReconciler,
    import_export, read_batch, read_import_manifest,
};
use hodei_artifacts_api::recovery::reconcile::Divergence;
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

fn options(batch_rows: usize, resume: bool) -> ImportOptions {
    ImportOptions {
        batch_rows,
        resume,
        ..ImportOptions::default()
    }
}

/// The CAS of the rehearsal platform: exactly the digests that the
/// fixture references, read from the package itself.
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

/// The origin with traffic: the fixture imported clean, 31 rows. Seeding
/// through the 210 seam is what gives realism -- the origin contains
/// exactly what the rehearsal's import is going to confront.
async fn seeded_origin() -> Arc<MemorySink> {
    let dir = tempfile::tempdir().unwrap();
    let sink: Arc<MemorySink> = Arc::new(MemorySink::new());
    let report = import_export(
        &package_dir(),
        sink.clone(),
        Some(fixture_cas()),
        &options(4, false),
        &dir.path().join(".seed-progress.json"),
    )
    .await
    .expect("the origin is seeded from the fixture");
    assert_eq!(report.created, 31);
    sink
}

/// Builds a machine over a seeded origin and an empty destination, already with the
/// barrier closed... no: not closed. It also returns the workspace, which
/// the caller must keep alive.
async fn rehearsal(
    workspace: &Path,
    cas: Arc<dyn DigestReconciler>,
    destination: Arc<dyn RowSink>,
    opts: ImportOptions,
) -> CutoverMachine {
    let origin = Arc::new(Authority::origin("origin", seeded_origin().await));
    let destination = Arc::new(Authority::destination("destination", destination));
    CutoverMachine::new(origin, destination, cas, workspace.to_path_buf(), opts)
        .expect("the rehearsal starts with origin active and destination on standby")
}

async fn default_machine() -> (tempfile::TempDir, CutoverMachine) {
    let dir = tempfile::tempdir().unwrap();
    let machine = rehearsal(
        dir.path(),
        fixture_cas(),
        Arc::new(MemorySink::new()),
        options(4, false),
    )
    .await;
    (dir, machine)
}

/// AC1 invariant: in any phase there is at most one active authority.
fn assert_single_authority(machine: &CutoverMachine) {
    assert!(
        !(machine.origin().is_active() && machine.destination().is_active()),
        "split-brain in phase {}",
        machine.phase().name()
    );
}

/// The happy path up to the cut, shared by the rehearsals.
async fn cut_over(machine: &mut CutoverMachine) -> hodei_artifacts_api::recovery::CutoverToken {
    assert_single_authority(machine);
    machine.close_barrier().await.expect("barrier");
    assert_eq!(machine.phase(), Phase::SnapshotTaken);
    machine.import_snapshot().await.expect("import");
    assert_eq!(machine.phase(), Phase::DestinationImported);
    let token = machine
        .verify()
        .await
        .expect("gate verifies the faithful import");
    assert_eq!(machine.phase(), Phase::GateVerified);
    // The API is not switched without this rehearsal's token.
    machine
        .commit(&token)
        .await
        .expect("switch with the real token");
    assert_eq!(machine.phase(), Phase::CutOver);
    assert_single_authority(machine);
    token
}

#[tokio::test]
async fn ac1_switch_leaves_origin_read_only_and_destination_exclusive() {
    let (_dir, mut machine) = default_machine().await;
    let token = cut_over(&mut machine).await;

    // The origin, frozen at the barrier, is strictly read-only: it serves
    // the 31 rows and rejects and counts each attempt of the old service.
    let sample = machine
        .origin()
        .read("catalog_tag:acme|main|stable")
        .await
        .unwrap();
    assert!(sample.is_some(), "the frozen origin still serves reads");
    let before = machine.origin().refused_writes();
    let mut stray = Row::new();
    stray.insert("account".into(), Value::String("acme".into()));
    stray.insert("repository".into(), Value::String("main".into()));
    stray.insert("tag_name".into(), Value::String("rogue".into()));
    stray.insert("target_version".into(), Value::String("9.9.9".into()));
    let error = machine
        .origin()
        .write("catalog_tag:acme|main|rogue", &stray)
        .await
        .expect_err("the frozen origin refuses service writes");
    assert!(matches!(error, AuthorityError::Frozen { .. }), "{error}");
    assert_eq!(
        machine.origin().refused_writes(),
        before + 1,
        "rechazo contado"
    );

    // Integrity/trust: the fingerprint verified at the cut is that of
    // both authorities, frozen in the token.
    assert_eq!(
        machine.destination().fingerprint().await.unwrap(),
        token.destination_fingerprint(),
    );
    assert_eq!(
        machine.origin().fingerprint().await.unwrap(),
        token.origin_fingerprint(),
    );
    assert_eq!(token.origin_fingerprint(), token.destination_fingerprint());

    // The destination is the exclusive authority: it writes with canonical
    // identity and reads what was imported. After the cut, the destination no longer
    // matches the token -- which proves it is the one in charge.
    machine
        .destination()
        .write("catalog_tag:acme|main|newest", &stray)
        .await
        .expect("the new authority accepts writes");
    assert!(
        machine
            .destination()
            .read("catalog_tag:acme|main|newest")
            .await
            .unwrap()
            .is_some()
    );
    assert_ne!(
        machine.destination().fingerprint().await.unwrap(),
        token.destination_fingerprint(),
        "a write on the new authority moves it past the verified token"
    );

    // There is no second switch without a new cycle: the phase no longer
    // allows it (no silent re-switch).
    let error = machine
        .commit(&token)
        .await
        .expect_err("a committed machine cannot switch again");
    assert!(
        matches!(&error, CutoverError::IllegalTransition { phase, .. } if *phase == Phase::CutOver.name()),
        "{error}"
    );
}

#[tokio::test]
async fn ac1_dirty_destination_blocks_the_gate_and_no_commit_happens() {
    let (_dir, mut machine) = default_machine().await;
    machine.close_barrier().await.expect("barrier");
    machine.import_snapshot().await.expect("import");

    // The service dirties the destination from outside the machine before
    // the verification: the gate names it, does not forgive it.
    let mut tag = Row::new();
    tag.insert("account".into(), Value::String("acme".into()));
    tag.insert("repository".into(), Value::String("main".into()));
    tag.insert("tag_name".into(), Value::String("drift".into()));
    tag.insert("target_version".into(), Value::String("0.0.1".into()));
    machine
        .destination()
        .store()
        .put("catalog_tag:acme|main|drift", &tag)
        .await
        .unwrap();

    let error = machine
        .verify()
        .await
        .expect_err("a divergent destination never verifies");
    let CutoverError::Blocked(divergences) = &error else {
        panic!("expected Blocked, got {error}");
    };
    assert!(
        divergences
            .iter()
            .any(|d| matches!(d, Divergence::Extra { what } if what.contains("drift"))),
        "the extra row must be named: {divergences:?}"
    );
    // Without a token there is no GateVerified phase, and therefore commit is illegal.
    assert_eq!(machine.phase(), Phase::DestinationImported);
    assert!(!machine.destination().is_active(), "el corte no ocurrió");
    assert!(machine.origin().is_frozen());
    assert_single_authority(&machine);
}

#[tokio::test]
async fn ac2_pre_write_rollback_restores_the_origin_exactly() {
    let (_dir, mut machine) = default_machine().await;
    let origin_fingerprint = machine
        .origin()
        .fingerprint()
        .await
        .expect("origin fingerprint before the rehearsal");
    let token = cut_over(&mut machine).await;

    // The destination drifts from outside: free rollback requires the state
    // exactly verified by the token.
    let mut tag = Row::new();
    tag.insert("account".into(), Value::String("acme".into()));
    tag.insert("repository".into(), Value::String("main".into()));
    tag.insert("tag_name".into(), Value::String("sneaky".into()));
    tag.insert("target_version".into(), Value::String("6.6.6".into()));
    machine
        .destination()
        .store()
        .put("catalog_tag:acme|main|sneaky", &tag)
        .await
        .unwrap();
    let error = machine
        .rollback()
        .await
        .expect_err("rollback over a drifted destination is refused");
    assert!(matches!(error, CutoverError::Drifted { .. }), "{error}");

    // Run the rollback over an intact state: another clean machine.
    let (_dir2, mut clean) = default_machine().await;
    cut_over(&mut clean).await;
    clean.rollback().await.expect("free pre-write rollback");
    assert_eq!(clean.phase(), Phase::RolledBack);
    assert!(
        clean.origin().is_active(),
        "the origin holds authority again"
    );
    assert!(!clean.destination().is_active(), "the destination is off");
    assert_eq!(
        clean.origin().fingerprint().await.unwrap(),
        origin_fingerprint,
        "restore is exact: the origin never wrote since the barrier"
    );
    // And the service writes to the origin again.
    let mut tag = Row::new();
    tag.insert("account".into(), Value::String("acme".into()));
    tag.insert("repository".into(), Value::String("main".into()));
    tag.insert("tag_name".into(), Value::String("after-rollback".into()));
    tag.insert("target_version".into(), Value::String("1.2.3".into()));
    clean
        .origin()
        .write("catalog_tag:acme|main|after-rollback", &tag)
        .await
        .expect("the restored origin takes writes again");
    let _ = token;
}

/// Three coherent new writes: a tag, its ledger event and the
/// head that backs it (prepare would reject a head that lies).
fn post_cutover_writes() -> Vec<(String, Row)> {
    let mut tag = Row::new();
    tag.insert("account".into(), Value::String("acme".into()));
    tag.insert("repository".into(), Value::String("main".into()));
    tag.insert("tag_name".into(), Value::String("beta".into()));
    tag.insert("target_version".into(), Value::String("2.1.0".into()));

    let mut event = Row::new();
    event.insert("stream".into(), Value::String("stream_a".into()));
    event.insert("position".into(), Value::from(3u64));
    event.insert("kind".into(), Value::String("tag.moved".into()));
    event.insert(
        "recorded_at".into(),
        Value::String("2026-11-22T09:00:00Z".into()),
    );

    let mut head = Row::new();
    head.insert("stream".into(), Value::String("stream_a".into()));
    head.insert("last_position".into(), Value::from(3u64));

    vec![
        ("catalog_tag:acme|main|beta".to_string(), tag),
        ("event_ledger:stream_a,3".to_string(), event),
        ("event_ledger_head:stream_a".to_string(), head),
    ]
}

#[tokio::test]
async fn ac2_post_write_forward_recovery_returns_without_silent_loss() {
    let (_dir, mut machine) = default_machine().await;
    let cut_token = cut_over(&mut machine).await;

    for (identity, row) in post_cutover_writes() {
        machine
            .write_new(&identity, &row)
            .await
            .expect("post-cutover write lands on the new authority");
    }
    assert_eq!(machine.phase(), Phase::CutOverLive);

    // Rule 2: after the first new write there is no free way back.
    let error = machine
        .rollback()
        .await
        .expect_err("free rollback is dead after the first new write");
    assert!(
        matches!(&error, CutoverError::PostWriteRollbackForbidden { writes } if *writes == 3),
        "{error}"
    );

    // Forward recovery: freeze, export, replay to the origin with gate.
    machine
        .export_forward()
        .await
        .expect("freeze + export the new authority");
    assert_eq!(machine.phase(), Phase::ReplayExported);
    let replay_token = machine
        .replay_to_origin()
        .await
        .expect("the replay verifies against the frozen destination");
    assert_eq!(machine.phase(), Phase::ReplayValidated);

    // The return requires the token of THIS replay: that of the original cut is no
    // longer valid -- its fingerprints describe another state.
    let error = machine
        .rollback_with_replay(&cut_token)
        .await
        .expect_err("the cutover token is not the replay token");
    assert!(matches!(error, CutoverError::StaleToken { .. }), "{error}");

    machine
        .rollback_with_replay(&replay_token)
        .await
        .expect("the validated replay token brings the authority home");
    assert_eq!(machine.phase(), Phase::RolledBack);
    assert!(machine.origin().is_active());
    assert!(!machine.destination().is_active());
    assert_single_authority(&machine);

    // Without silent loss: the three new writes are in the
    // restored origin.
    for (identity, _) in post_cutover_writes() {
        assert!(
            machine.origin().read(&identity).await.unwrap().is_some(),
            "the replay lost '{identity}'"
        );
    }
    assert_eq!(
        machine.origin().fingerprint().await.unwrap(),
        replay_token.origin_fingerprint(),
        "the origin is exactly where the replay gate verified it"
    );
}

#[tokio::test]
async fn ac2_a_kill_mid_import_resumes_to_the_identical_fingerprint() {
    // AC2/AC1: the import leg survives a cut: write budget exhausted midway,
    // resume, and the destination ends at the
    // fingerprint identical to that of the clean run (CrashSink 210 pattern).
    let dir = tempfile::tempdir().unwrap();
    let inner = Arc::new(MemorySink::new());
    let budget = Arc::new(AtomicUsize::new(5));
    let destination = Arc::new(CrashSink {
        inner: inner.clone(),
        budget: budget.clone(),
    });
    let mut machine = rehearsal(
        dir.path(),
        fixture_cas(),
        destination,
        options(1, true), // rows of 1 and resume: the cut falls inside a batch
    )
    .await;

    machine.close_barrier().await.expect("barrier");
    let origin_fingerprint = machine.origin().fingerprint().await.unwrap();
    let error = machine
        .import_snapshot()
        .await
        .expect_err("the kill breaks the first pass");
    assert!(
        matches!(&error, CutoverError::Import(ImportFailure::Store(_))),
        "{error}"
    );
    assert_eq!(
        machine.phase(),
        Phase::SnapshotTaken,
        "the leg failed, the phase did not move"
    );
    assert!(dir.path().join(".cutover-import-progress.json").exists());

    // The operator recovers the process and resumes: renewed budget.
    budget.store(usize::MAX, Ordering::SeqCst);
    machine
        .import_snapshot()
        .await
        .expect("the resume completes the import");
    assert_eq!(
        machine.destination().fingerprint().await.unwrap(),
        origin_fingerprint,
        "the resumed destination equals the clean one row for row"
    );
    assert_eq!(inner.len().await, 31, "the kill added no duplicate");

    // And the rehearsal continues up to the cut: the resumption did not leave the
    // state in no-man's-land.
    let token = machine
        .verify()
        .await
        .expect("the gate passes after the resume");
    machine
        .commit(&token)
        .await
        .expect("switch after the resume");
    assert_single_authority(&machine);
}

/// The sink that "dies" after N writes, as in HSP-210.
struct CrashSink {
    inner: Arc<MemorySink>,
    budget: Arc<AtomicUsize>,
}

#[async_trait]
impl RowSink for CrashSink {
    async fn lookup(
        &self,
        identity: &str,
    ) -> Result<Option<Row>, hodei_artifacts_api::recovery::RecoveryError> {
        self.inner.lookup(identity).await
    }

    async fn put(
        &self,
        identity: &str,
        row: &Row,
    ) -> Result<(), hodei_artifacts_api::recovery::RecoveryError> {
        let left = self
            .budget
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                if n == 0 { None } else { Some(n - 1) }
            });
        if left.is_err() {
            return Err(hodei_artifacts_api::recovery::RecoveryError::Store(
                "simulated kill".to_string(),
            ));
        }
        self.inner.put(identity, row).await
    }

    fn fingerprint_row(&self, row: &Row) -> String {
        self.inner.fingerprint_row(row)
    }

    async fn state_summary(
        &self,
    ) -> Result<
        hodei_artifacts_api::recovery::DestinationState,
        hodei_artifacts_api::recovery::RecoveryError,
    > {
        self.inner.state_summary().await
    }
}

#[tokio::test]
async fn ac3_window_measured_and_rpo_rto_published_as_observed() {
    let dir = tempfile::tempdir().unwrap();
    let mut machine = rehearsal(
        dir.path(),
        fixture_cas(),
        Arc::new(MemorySink::new()),
        options(4, false),
    )
    .await;

    // Real traffic during the window: an attempt by the old service
    // while the barrier is closed. It is rejected and counted: the RPO
    // is reported measured, not sworn.
    machine.close_barrier().await.expect("barrier");
    let mut stray = Row::new();
    stray.insert("account".into(), Value::String("acme".into()));
    stray.insert("repository".into(), Value::String("main".into()));
    stray.insert("tag_name".into(), Value::String("lost-attempt".into()));
    stray.insert("target_version".into(), Value::String("0.0.0".into()));
    for _ in 0..3 {
        let error = machine
            .origin()
            .write("catalog_tag:acme|main|lost-attempt", &stray)
            .await
            .expect_err("the barrier holds");
        assert!(matches!(error, AuthorityError::Frozen { .. }));
    }

    machine.import_snapshot().await.expect("import");
    let token = machine.verify().await.expect("gate");
    machine.commit(&token).await.expect("switch");

    // First new write measured (the RTO ends at the cut; this
    // is the resumed traffic that confirms it alive).
    machine
        .write_new("catalog_tag:acme|main|first-live", &stray)
        .await
        .expect("the new authority serves the first live write");

    let metrics = machine.metrics();
    assert!(
        metrics.window.total.as_nanos() > 0,
        "the window was timed, not stubbed"
    );
    assert_eq!(
        metrics.origin_refused, 3,
        "each refused write is counted, not estimated"
    );
    assert_eq!(metrics.rpo_rows_observed(), 0);
    assert_eq!(metrics.new_writes, 1);

    let evidence = machine.evidence();
    assert!(evidence.contains("RPO observado: 0 filas"), "{evidence}");
    assert!(evidence.contains("RTO observado"), "{evidence}");
    assert!(evidence.contains(&metrics.verified_fingerprint));

    // Publish the evidence (AC3): measured values of this run.
    let tmp = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    let target = tmp.parent().unwrap_or(&tmp).to_path_buf();
    let dir = target.join("uat").join("hsp212");
    std::fs::create_dir_all(&dir).expect("the evidence directory");
    std::fs::write(dir.join("evidencia.md"), &evidence).expect("the evidence is published");
    eprintln!("HSP212 evidence at {}", dir.join("evidencia.md").display());
    eprint!("{evidence}");
}
