//! The collection service, over a real store and an in-memory ledger.
//!
//! The store is the real file-based one (the barrier, the `mtime` grace
//! and the deletion are real); the only thing simulated is the ledger,
//! so that cuts can be provoked between the mark, the deletion and the
//! confirmation.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_artifacts_api::gc::{GcError, GcService, Verdict};
use hodei_catalog::{
    CatalogError, ContentHold, ContentLedger, ContentRoot, DeletionState, GcDeletion, GcRun,
    HoldScope, RootKind, RunCandidate, RunState,
};
use hodei_evidence::ContentDigest;
use hodei_hub::{BlobStore, FileBlobStore};

const GRACE: Duration = Duration::from_millis(900);

#[derive(Default)]
struct State {
    roots: Vec<ContentRoot>,
    holds: Vec<ContentHold>,
    runs: Vec<GcRun>,
    tombstones: Vec<GcDeletion>,
    audit: Vec<String>,
}

#[derive(Default)]
struct Ledger {
    state: Mutex<State>,
}

#[async_trait]
impl ContentLedger for Ledger {
    async fn content_roots(&self) -> Result<Vec<ContentRoot>, CatalogError> {
        Ok(self.state.lock().unwrap().roots.clone())
    }
    async fn holds(&self) -> Result<Vec<ContentHold>, CatalogError> {
        Ok(self.state.lock().unwrap().holds.clone())
    }
    async fn place_hold(&self, hold: &ContentHold) -> Result<(), CatalogError> {
        let mut state = self.state.lock().unwrap();
        state.holds.retain(|existing| existing.id != hold.id);
        state.holds.push(hold.clone());
        Ok(())
    }
    async fn release_hold(&self, id: &str) -> Result<bool, CatalogError> {
        let mut state = self.state.lock().unwrap();
        let before = state.holds.len();
        state.holds.retain(|hold| hold.id != id);
        Ok(state.holds.len() != before)
    }
    async fn pending_run(&self) -> Result<Option<GcRun>, CatalogError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .runs
            .iter()
            .find(|run| run.state == RunState::Marked)
            .cloned())
    }
    async fn last_swept_run(&self) -> Result<Option<GcRun>, CatalogError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .runs
            .iter()
            .rev()
            .find(|run| run.state == RunState::Swept)
            .cloned())
    }
    async fn open_run(&self, run: &GcRun) -> Result<(), CatalogError> {
        let mut state = self.state.lock().unwrap();
        state.runs.retain(|existing| existing.id != run.id);
        state.runs.push(run.clone());
        Ok(())
    }
    async fn complete_run(
        &self,
        run_id: &str,
        deleted: u64,
        bytes_freed: u64,
        at: DateTime<Utc>,
    ) -> Result<(), CatalogError> {
        let mut state = self.state.lock().unwrap();
        for run in state.runs.iter_mut().filter(|run| run.id == run_id) {
            run.state = RunState::Swept;
            run.deleted = deleted;
            run.bytes_freed = bytes_freed;
            run.swept_at = Some(at);
        }
        Ok(())
    }
    async fn record_deletion(&self, deletion: &GcDeletion) -> Result<(), CatalogError> {
        let mut state = self.state.lock().unwrap();
        let already = state.tombstones.iter().any(|existing| {
            existing.digest == deletion.digest
                && existing.state == DeletionState::Confirmed
                && existing.run_id == deletion.run_id
        });
        if deletion.state == DeletionState::Confirmed && !already {
            state.audit.push(format!("gc-delete-{}", deletion.digest));
        }
        state
            .tombstones
            .retain(|existing| existing.digest != deletion.digest);
        state.tombstones.push(deletion.clone());
        Ok(())
    }
    async fn deletion_of(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<GcDeletion>, CatalogError> {
        let state = self.state.lock().unwrap();
        Ok(state
            .tombstones
            .iter()
            .find(|existing| &existing.digest == digest)
            .cloned())
    }
}

struct World {
    _dir: tempfile::TempDir,
    store: Arc<FileBlobStore>,
    ledger: Arc<Ledger>,
    service: GcService,
}

fn world() -> World {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Arc::new(FileBlobStore::new(dir.path().join("cas")).expect("store"));
    let ledger = Arc::new(Ledger::default());
    let service = GcService::new(ledger.clone(), store.clone(), GRACE).expect("service");
    World {
        _dir: dir,
        store,
        ledger,
        service,
    }
}

async fn put(world: &World, bytes: &[u8]) -> ContentDigest {
    world.store.put(bytes).await.expect("put")
}

fn root(kind: RootKind, repository: &str, subject: &str, digest: &ContentDigest) -> ContentRoot {
    ContentRoot {
        kind,
        account: "acme".to_string(),
        repository: repository.to_string(),
        subject: subject.to_string(),
        digest: digest.clone(),
    }
}

async fn age() {
    tokio::time::sleep(GRACE + Duration::from_millis(300)).await;
}

fn verdict<'a>(plan: &'a hodei_artifacts_api::gc::Plan, digest: &ContentDigest) -> &'a Verdict {
    let key = digest.to_string();
    &plan
        .entries
        .iter()
        .find(|entry| entry.digest == key)
        .expect("entry")
        .verdict
}

fn manifest(config: &ContentDigest, layers: &[&ContentDigest]) -> Vec<u8> {
    let layers: Vec<String> = layers
        .iter()
        .map(|digest| format!(r#"{{"digest":"{digest}"}}"#))
        .collect();
    format!(
        r#"{{"config":{{"digest":"{config}"}},"layers":[{}]}}"#,
        layers.join(",")
    )
    .into_bytes()
}

/// GC-01: what is shared by two repositories and the layers of a manifest
/// are not deleted; the old orphan is.
#[tokio::test]
async fn shared_blobs_and_oci_layers_survive_and_the_orphan_goes() {
    let w = world();
    let shared = put(&w, b"shared blob").await;
    let config = put(&w, b"config").await;
    let layer = put(&w, b"layer").await;
    let orphan = put(&w, b"orphan").await;
    let manifest_bytes = manifest(&config, &[&layer, &shared]);
    let manifest_digest = put(&w, &manifest_bytes).await;
    {
        let mut state = w.ledger.state.lock().unwrap();
        state.roots = vec![
            root(RootKind::Asset, "app", "blobs/shared", &shared),
            root(RootKind::Asset, "lib", "blobs/shared", &shared),
            root(
                RootKind::Asset,
                "app",
                &format!("manifests/{}", manifest_digest.value),
                &manifest_digest,
            ),
        ];
    }
    age().await;

    let report = w.service.sweep("ops", "acme").await.expect("sweep");
    assert_eq!(report.deleted, 1);
    assert!(!w.store.exists(&orphan).await.unwrap());
    for kept in [&shared, &config, &layer, &manifest_digest] {
        assert!(w.store.exists(kept).await.unwrap(), "{kept} must survive");
        w.store.verify(kept).await.expect("intact");
    }
}

/// GC-02: what is recent, what is retained and what a live manifest names are not
/// reclaimable, and the dry-run says why without deleting anything.
#[tokio::test]
async fn the_dry_run_explains_and_deletes_nothing() {
    let w = world();
    let held = put(&w, b"held").await;
    let old_orphan = put(&w, b"old orphan").await;
    age().await;
    let young = put(&w, b"young upload").await;
    let hold = w
        .service
        .place_hold(
            "ops",
            HoldScope::Digest {
                digest: held.clone(),
            },
            "legal hold",
            None,
        )
        .await
        .expect("hold");

    let plan = w.service.dry_run().await.expect("plan");
    assert!(matches!(verdict(&plan, &young), Verdict::InGrace { .. }));
    assert!(matches!(
        verdict(&plan, &old_orphan),
        Verdict::Reclaimable { .. }
    ));
    match verdict(&plan, &held) {
        Verdict::Retained { reasons, .. } => {
            let json = serde_json::to_string(reasons).unwrap();
            assert!(json.contains(&hold.id), "the cause names the hold: {json}");
        }
        other => panic!("expected retained, got {other:?}"),
    }
    assert_eq!(plan.summary.reclaimable_bytes, b"old orphan".len() as u64);
    for digest in [&held, &old_orphan, &young] {
        assert!(
            w.store.exists(digest).await.unwrap(),
            "dry-run deleted {digest}"
        );
    }
    assert!(
        w.ledger.state.lock().unwrap().runs.is_empty(),
        "no run in dry-run"
    );
}

/// GC-03: the sweep deletes the orphan exactly once and audits it once.
#[tokio::test]
async fn sweeping_twice_deletes_and_audits_once() {
    let w = world();
    let orphan = put(&w, b"orphan").await;
    age().await;

    let first = w.service.sweep("ops", "acme").await.expect("first");
    let second = w.service.sweep("ops", "acme").await.expect("second");
    assert_eq!((first.deleted, second.deleted), (1, 0));
    assert!(!w.store.exists(&orphan).await.unwrap());
    let state = w.ledger.state.lock().unwrap();
    assert_eq!(state.audit.len(), 1);
    let trace = state.tombstones.first().expect("trace");
    assert_eq!(trace.state, DeletionState::Confirmed);
    assert_eq!(trace.actor, "ops");
}

/// GC-04 (1/3): cut after marking and before deleting. The restart resumes
/// the marked pass, it does not open another.
#[tokio::test]
async fn a_cut_after_the_mark_is_resumed() {
    let w = world();
    let orphan = put(&w, b"orphan").await;
    age().await;
    let run = GcRun {
        id: "gc-cut".to_string(),
        state: RunState::Marked,
        grace_secs: GRACE.as_secs(),
        actor: "ops".to_string(),
        candidates: vec![RunCandidate {
            digest: orphan.clone(),
            length: 6,
        }],
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: None,
        generation: None,
    };
    w.ledger.open_run(&run).await.unwrap();

    let report = w.service.sweep("ops", "acme").await.expect("resume");
    assert!(report.resumed);
    assert_eq!(report.run_id, "gc-cut");
    assert_eq!(report.deleted, 1);
    assert!(!w.store.exists(&orphan).await.unwrap());
    assert!(w.ledger.pending_run().await.unwrap().is_none());
}

/// GC-04 (2/3): cut between the deletion and the confirmation. The restart sees
/// the intent and the missing blob: it confirms, and does not delete twice.
#[tokio::test]
async fn a_cut_between_delete_and_confirm_is_confirmed_once() {
    let w = world();
    let orphan = put(&w, b"orphan").await;
    age().await;
    let run = GcRun {
        id: "gc-cut".to_string(),
        state: RunState::Marked,
        grace_secs: GRACE.as_secs(),
        actor: "ops".to_string(),
        candidates: vec![RunCandidate {
            digest: orphan.clone(),
            length: 6,
        }],
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: None,
        generation: None,
    };
    w.ledger.open_run(&run).await.unwrap();
    w.ledger
        .record_deletion(&GcDeletion {
            digest: orphan.clone(),
            length: 6,
            run_id: "gc-cut".to_string(),
            actor: "ops".to_string(),
            account: "acme".to_string(),
            state: DeletionState::Intended,
            at: Utc::now(),
            marker: None,
        })
        .await
        .unwrap();
    // The deletion happened and the process died before confirming.
    let reclaimer = w.store.reclaimer().expect("reclaimer");
    reclaimer
        .delete_if_older_than(&orphan, GRACE)
        .await
        .unwrap();

    let report = w.service.sweep("ops", "acme").await.expect("resume");
    assert_eq!(report.deleted, 1);
    let state = w.ledger.state.lock().unwrap();
    assert_eq!(state.audit.len(), 1, "one audit event");
    assert_eq!(state.tombstones[0].state, DeletionState::Confirmed);
}

/// GC-04 (3/3): cut after confirming and before closing the pass. Resuming
/// repeats neither the deletion nor the event.
#[tokio::test]
async fn a_cut_after_the_confirmation_does_not_repeat_the_audit() {
    let w = world();
    let orphan = put(&w, b"orphan").await;
    age().await;
    let report = w.service.sweep("ops", "acme").await.expect("sweep");
    // The pass is reopened as if the cut had happened before closing it.
    {
        let mut state = w.ledger.state.lock().unwrap();
        state.runs[0].state = RunState::Marked;
    }
    let again = w.service.sweep("ops", "acme").await.expect("resume");
    assert!(again.resumed);
    assert_eq!(again.deleted, report.deleted);
    assert_eq!(w.ledger.state.lock().unwrap().audit.len(), 1);
    assert!(!w.store.exists(&orphan).await.unwrap());
}

/// Something published after marking is not deleted: the second scan
/// sees it alive.
#[tokio::test]
async fn what_is_published_after_the_mark_is_not_deleted() {
    let w = world();
    let saved = put(&w, b"published late").await;
    age().await;
    let run = GcRun {
        id: "gc-late".to_string(),
        state: RunState::Marked,
        grace_secs: GRACE.as_secs(),
        actor: "ops".to_string(),
        candidates: vec![RunCandidate {
            digest: saved.clone(),
            length: 14,
        }],
        marked_at: Utc::now(),
        deleted: 0,
        bytes_freed: 0,
        swept_at: None,
        scope: None,
        generation: None,
    };
    w.ledger.open_run(&run).await.unwrap();
    w.ledger.state.lock().unwrap().roots = vec![root(RootKind::Asset, "app", "blobs/late", &saved)];

    let report = w.service.sweep("ops", "acme").await.expect("sweep");
    assert_eq!((report.deleted, report.skipped), (0, 1));
    assert!(w.store.exists(&saved).await.unwrap());
}

/// UNKNOWN: a manifest that cannot be read aborts the whole pass and
/// nothing is deleted, not even what is clearly orphaned.
#[tokio::test]
async fn an_unreadable_manifest_aborts_without_deleting() {
    let w = world();
    let orphan = put(&w, b"orphan").await;
    let missing = ContentDigest::sha256("f".repeat(64));
    w.ledger.state.lock().unwrap().roots = vec![root(
        RootKind::Asset,
        "app",
        &format!("manifests/{}", missing.value),
        &missing,
    )];
    age().await;

    assert!(matches!(
        w.service.dry_run().await,
        Err(GcError::Unknown(_))
    ));
    assert!(matches!(
        w.service.sweep("ops", "acme").await,
        Err(GcError::Unknown(_))
    ));
    assert!(w.store.exists(&orphan).await.unwrap());
    assert!(w.ledger.state.lock().unwrap().tombstones.is_empty());
}

/// A manifest that is not understood does not block the collection of everything: it
/// contributes no edges, its content stays protected by its root and the orphan
/// is reclaimed. A tenant with a raw `manifests/<digest>` path cannot
/// leave the platform in an unknown state.
#[tokio::test]
async fn a_manifest_that_is_not_understood_does_not_block_the_collection() {
    let w = world();
    let orphan = put(&w, b"orphan").await;
    let junk = put(&w, b"this is not a manifest").await;
    w.ledger.state.lock().unwrap().roots = vec![root(
        RootKind::Asset,
        "app",
        &format!("manifests/{}", junk.value),
        &junk,
    )];
    age().await;
    let report = w.service.sweep("ops", "acme").await.expect("sweep");
    assert_eq!(report.deleted, 1);
    assert!(!w.store.exists(&orphan).await.unwrap());
    assert!(w.store.exists(&junk).await.unwrap(), "the root keeps it");
}

/// A raw path that merely starts with `manifests/` is not a manifest: it is neither
/// read nor can it abort the pass.
#[tokio::test]
async fn a_raw_path_under_manifests_is_not_read_as_a_manifest() {
    let w = world();
    let orphan = put(&w, b"orphan").await;
    let file = put(&w, b"plain bytes").await;
    w.ledger.state.lock().unwrap().roots =
        vec![root(RootKind::Asset, "files", "manifests/x", &file)];
    age().await;
    let report = w.service.sweep("ops", "acme").await.expect("sweep");
    assert_eq!(report.deleted, 1);
    assert!(!w.store.exists(&orphan).await.unwrap());
    assert!(w.store.exists(&file).await.unwrap());
}

#[tokio::test]
async fn holds_validate_their_input_and_can_be_released() {
    let w = world();
    let digest = put(&w, b"x").await;
    let scope = HoldScope::Digest { digest };
    assert!(matches!(
        w.service.place_hold("ops", scope.clone(), "  ", None).await,
        Err(GcError::Invalid(_))
    ));
    let past = Utc::now() - chrono::Duration::seconds(5);
    assert!(matches!(
        w.service
            .place_hold("ops", scope.clone(), "why", Some(past))
            .await,
        Err(GcError::Invalid(_))
    ));
    let hold = w
        .service
        .place_hold("ops", scope, "why", None)
        .await
        .unwrap();
    assert!(w.service.release_hold(&hold.id).await.unwrap());
    assert!(!w.service.release_hold(&hold.id).await.unwrap());
}

/// A corrupt or missing OCI manifest does not make the scrub abort: it is
/// exactly what it has to report (unlike the collection,
/// which does not act on an unreadable manifest).
#[tokio::test]
async fn a_damaged_or_missing_oci_manifest_is_reported_by_the_scrub_not_fatal() {
    use hodei_artifacts_api::recovery::{Finding, Scrubber};

    fn find(dir: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
        for entry in std::fs::read_dir(dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = find(&path, name) {
                    return Some(found);
                }
            } else if path.file_name().is_some_and(|n| n == name) {
                return Some(path);
            }
        }
        None
    }

    let w = world();
    let config = put(&w, b"config").await;
    let layer = put(&w, b"layer").await;
    let damaged = put(&w, &manifest(&config, &[&layer])).await;
    let other_layer = put(&w, b"another layer").await;
    let missing = put(&w, &manifest(&config, &[&other_layer])).await;
    {
        let mut state = w.ledger.state.lock().unwrap();
        state.roots = vec![
            root(
                RootKind::Asset,
                "app",
                &format!("manifests/{}", damaged.value),
                &damaged,
            ),
            root(
                RootKind::Asset,
                "app",
                &format!("manifests/{}", missing.value),
                &missing,
            ),
        ];
    }
    let cas = w._dir.path().join("cas");
    let file = find(&cas, &damaged.value).expect("the manifest file");
    std::fs::write(&file, b"not the manifest anymore").unwrap();
    let gone = find(&cas, &missing.value).expect("the manifest file");
    std::fs::remove_file(gone).unwrap();

    // The collection does not act on an unreadable manifest...
    assert!(w.service.dry_run().await.is_err());

    // ...and the scrub reports it instead of aborting.
    let scrubber = Scrubber::new(w.ledger.clone(), w.store.clone(), GRACE).expect("scrubber");
    let report = scrubber
        .run()
        .await
        .expect("the scrub reports, it does not fail");
    assert!(!report.is_sound());
    assert!(report.findings.iter().any(
        |finding| matches!(finding, Finding::Corrupt { digest, .. } if *digest == damaged.to_string())
    ));
    assert!(report.findings.iter().any(
        |finding| matches!(finding, Finding::BrokenReference { digest, .. } if *digest == missing.to_string())
    ));
}
