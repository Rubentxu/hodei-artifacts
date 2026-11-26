//! HSP-201: resumable OCI sessions between two replicas over the
//! real file store.
//!
//! The slice takes the session registry out of the process memory. This
//! cut tests it without a server: a shared **journal** (the offset
//! authority), two **registries** -- the two replicas -- and a real
//! [`FileBlobStore`] in a temporary directory.
//!
//! - **AC1**: an upload opened on replica A continues on replica
//!   B, and the closing `PUT` is done on B: the offset and the digest of the
//!   published blob are the usual ones. A stale writer (replay) does not
//!   move the authority; a staging with bytes above the authority
//!   is trimmed on resume, and a close whose length does not match
//!   publishes nothing.
//! - **AC2**: a session of another account does not exist for this one: it is neither
//!   resumed nor is its offset read from another scope.
//! - **AC3**: the TTL sweep closes the authority and aborts the orphan
//!   staging of a session that another replica abandoned, without ever publishing
//!   unverified bytes.
//!
//! The Dgraph journal (the same authority over the real graph, with its
//! CAS arbitrated by the server and its sweep bounded to the tenant) is
//! tested in `tests/hsp201_sessions_dgraph_it.rs`.

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use hodei_artifacts_api::oci::session_journal::{MemoryJournal, SessionJournal};
use hodei_artifacts_api::oci::uploads::{Advance, UploadRegistry};
use hodei_artifacts_api::quota::{QuotaLedger, QuotaLimits};
use hodei_evidence::ContentDigest;
use hodei_hub::blob::{BlobStore, CommitExpectation, FileBlobStore};

/// The content of a chunk: twenty deterministic kilobytes.
fn a_piece(seed: u8) -> Vec<u8> {
    (0u16..20_480)
        .map(|value| ((value as u8).wrapping_add(seed)) % 251)
        .collect()
}

/// A reservation without limits: these tests test sessions, not quotas.
fn an_unlimited_ticket(account: &str) -> hodei_artifacts_api::quota::QuotaTicket {
    QuotaLedger::new(QuotaLimits::default())
        .begin(account, None)
        .expect("an unlimited ledger admits everything")
}

/// The declared digest of a content.
fn a_declared_digest(content: &[u8]) -> ContentDigest {
    ContentDigest::of_bytes(content)
}

/// Opens a session on the replica, writes a chunk and confesses it to the
/// authority: the `PATCH` path, minus the wire.
async fn write_a_piece(
    registry: &UploadRegistry,
    store: &FileBlobStore,
    account: &str,
    repository: &str,
    id: &str,
    version: u64,
    piece: &[u8],
) -> u64 {
    let session = registry
        .find_or_resume(account, repository, id, store)
        .await
        .expect("the session exists")
        .expect("the session is of this scope");
    let durable = {
        let mut session = session.lock().await;
        let (writer, _ticket) = session
            .the_writer_and_ticket()
            .expect("the session has a writer");
        writer.append(piece).await.expect("the piece is written");
        session.flush_durable().await.expect("the piece is durable")
    };
    match registry.advance(id, version, durable).await {
        Advance::Admitted(next) => {
            let mut session = session.lock().await;
            session.set_journal_version(next);
            // The replica lets go of the handle: the staging belongs to the journal, and
            // the next request -- here or on the other replica -- takes it.
            session.release_writer_when_possible();
            next
        }
        other => panic!("the authority refused the piece: {other:?}"),
    }
}

/// AC1 — what replica A started, replica B finishes.
#[tokio::test]
async fn two_replicas_publish_one_blob_through_the_authority() {
    let root = tempfile::tempdir().expect("a scratch root");
    let store = FileBlobStore::new(root.path()).expect("the file store");
    let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
    let replica_a =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));
    let replica_b =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));

    let first = a_piece(1);
    let second = a_piece(2);
    let whole = [first.as_slice(), second.as_slice()].concat();

    // POST on A: the session is born in the authority and the handle is released.
    let session = replica_a
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let id = session.lock().await.the_id().to_string();
    drop(session);

    // PATCH of the first chunk on A: it writes, the authority admits, the
    // handle is released again.
    write_a_piece(&replica_a, &store, "acme", "releases", &id, 0, &first).await;

    // PATCH of the second chunk on B: B has no seat; the authority
    // states the offset and the store reopens the staging.
    write_a_piece(&replica_b, &store, "acme", "releases", &id, 1, &second).await;

    // Closing PUT on B: the last handle commits against the declared
    // digest. The session started in another process, so the local digest
    // does not cover the first bytes; the one that publishes is the store,
    // and the promise does not change.
    let shared = replica_b
        .find_or_resume("acme", "releases", &id, &store)
        .await
        .expect("the session exists")
        .expect("the session is of this scope");
    let published = {
        let mut session = shared.lock().await;
        let writer = session.the_writer().expect("the session has a writer");
        writer
            .commit(CommitExpectation {
                digest: a_declared_digest(&whole),
                length: whole.len() as u64,
            })
            .await
            .expect("the commit publishes the whole blob")
    };
    assert_eq!(published, a_declared_digest(&whole));
    assert!(
        store.exists(&published).await.expect("the store answers"),
        "the blob is not there after a cross-replica publish"
    );
    assert_eq!(
        store.get(&published).await.expect("the blob reads"),
        whole,
        "the published bytes are not the bytes the two replicas wrote"
    );

    // The authority stayed at the final offset and the session is closed.
    let record = journal
        .get(&id)
        .await
        .expect("the journal answers")
        .expect("the session stands");
    assert_eq!(record.offset, whole.len() as u64);
    assert_eq!(record.version, 2);
    replica_b.close(&id).await.expect("the session closes");
    let record = journal
        .get(&id)
        .await
        .expect("the journal answers")
        .expect("the session stands closed");
    assert!(record.closed);
    assert!(
        replica_b
            .find_or_resume("acme", "releases", &id, &store)
            .await
            .expect("the journal answers")
            .is_none()
    );
}

/// AC1 — a stale writer does not move the authority and a staging above
/// it does not duplicate bytes when publishing.
#[tokio::test]
async fn a_stale_replay_never_duplicates_bytes() {
    let root = tempfile::tempdir().expect("a scratch root");
    let store = FileBlobStore::new(root.path()).expect("the file store");
    let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
    let replica_a =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));
    let replica_b =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));

    let first = a_piece(1);
    let second = a_piece(2);

    let session = replica_a
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let id = session.lock().await.the_id().to_string();
    drop(session);

    // The first chunk is confessed: authority at `first`, revision 1.
    write_a_piece(&replica_a, &store, "acme", "releases", &id, 0, &first).await;

    // A replay with the old revision moves nothing: it is the protocol's
    // `416`, and the authority stays where it was.
    match replica_a.advance(&id, 0, 999_999).await {
        Advance::Conflict(offset) => assert_eq!(offset, first.len() as u64),
        other => panic!("the stale writer was not refused: {other:?}"),
    }

    // And the bytes above the authority go away on resume: a
    // staging longer than declared is trimmed, not published.
    // The handle comes from the authority, not from a seat: replica A
    // already let go of its handle and does not take it again.
    let handle = journal
        .get(&id)
        .await
        .expect("the journal answers")
        .expect("the session stands")
        .handle;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&handle)
        .expect("the staging opens")
        .write_all(&second)
        .expect("a stray piece is scribbled past the authority");

    // Replica B resumes with the authority as truth: what is left over
    // is cut and the cursor B sees is the authority's.
    let shared = replica_b
        .find_or_resume("acme", "releases", &id, &store)
        .await
        .expect("the session exists")
        .expect("the session is of this scope");
    let (cursor, covers) = {
        let session = shared.lock().await;
        (
            session
                .the_local_offset()
                .expect("the session has a local offset"),
            session.the_digest_covers_all(),
        )
    };
    assert_eq!(
        cursor,
        first.len() as u64,
        "the resume did not trim the stray bytes"
    );
    assert!(
        !covers,
        "a resumed session cannot claim a full local digest"
    );
    let published = {
        let mut session = shared.lock().await;
        let writer = session.the_writer().expect("the session resumed");
        writer
            .append(&second)
            .await
            .expect("the second piece is written");
        writer
            .commit(CommitExpectation {
                digest: a_declared_digest(&[first.as_slice(), second.as_slice()].concat()),
                length: first.len() as u64 + second.len() as u64,
            })
            .await
            .expect("the commit publishes exactly the authority's bytes")
    };
    assert_eq!(
        store.get(&published).await.expect("the blob reads"),
        [first.as_slice(), second.as_slice()].concat(),
        "a replayed piece reached the published blob"
    );

    // A close whose length does not match does not publish: the store discards.
    let session = replica_b
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let other_id = session.lock().await.the_id().to_string();
    drop(session);
    write_a_piece(&replica_b, &store, "acme", "releases", &other_id, 0, &first).await;
    let shared = replica_b
        .find_or_resume("acme", "releases", &other_id, &store)
        .await
        .expect("the session exists")
        .expect("the session is of this scope");
    let wrong = {
        let mut session = shared.lock().await;
        let writer = session.the_writer().expect("the session has a writer");
        writer
            .commit(CommitExpectation {
                digest: a_declared_digest(&first),
                length: first.len() as u64 + 7,
            })
            .await
    };
    assert!(
        wrong.is_err(),
        "a commit whose length does not match must not publish"
    );
    assert!(
        !store
            .exists(&a_declared_digest(&first))
            .await
            .expect("the store answers"),
        "a rejected commit left a published blob behind"
    );
}

/// AC2 — a session of another account does not exist: it is neither resumed nor read.
#[tokio::test]
async fn an_alien_session_is_invisible() {
    let root = tempfile::tempdir().expect("a scratch root");
    let store = FileBlobStore::new(root.path()).expect("the file store");
    let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
    let replica_a =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));
    let replica_b =
        UploadRegistry::with_journal(Duration::from_secs(3600), 8, Arc::clone(&journal));

    let session = replica_a
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let id = session.lock().await.the_id().to_string();
    drop(session);

    // Another account does not see it -- not even on the other replica -- and its offset
    // is not readable from its scope either.
    assert!(
        replica_b
            .find_or_resume("other", "releases", &id, &store)
            .await
            .expect("the journal answers")
            .is_none()
    );
    assert!(
        replica_b
            .find_or_resume("acme", "other-repo", &id, &store)
            .await
            .expect("the journal answers")
            .is_none()
    );
    assert_eq!(
        replica_b
            .the_journal_offset("other", "releases", &id)
            .await
            .expect("the journal answers"),
        None
    );
    // The owner does see it.
    assert!(
        replica_b
            .find_or_resume("acme", "releases", &id, &store)
            .await
            .expect("the journal answers")
            .is_some()
    );
}

/// AC3 — the sweep reconciles the orphan staging and publishes nothing.
#[tokio::test]
async fn the_ttl_sweep_aborts_the_orphan_without_publishing() {
    let root = tempfile::tempdir().expect("a scratch root");
    let store = FileBlobStore::new(root.path()).expect("the file store");
    let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
    // A grace of a few milliseconds: the upload never finishes.
    let ttl = Duration::from_millis(20);
    let replica_a = UploadRegistry::with_journal(ttl, 8, Arc::clone(&journal));
    // The "other replica" is the same journal without seats: it is the
    // authority that survived the crash.
    let replica_b = UploadRegistry::with_journal(ttl, 8, Arc::clone(&journal));

    let piece = a_piece(3);
    let session = replica_a
        .open(
            "acme",
            "releases",
            store.begin().await.expect("a writer"),
            an_unlimited_ticket("acme"),
        )
        .await
        .expect("the session opens");
    let id = session.lock().await.the_id().to_string();
    drop(session);
    write_a_piece(&replica_a, &store, "acme", "releases", &id, 0, &piece).await;

    // The crash: replica B waits for the grace to pass and sweeps.
    tokio::time::sleep(Duration::from_millis(80)).await;
    let reconciled = replica_b.sweep(&store).await.expect("the sweep answers");
    assert!(
        reconciled >= 1,
        "the orphan of a dead replica was not reconciled"
    );

    // Nothing published: the staging died and the blob does not exist.
    assert!(
        !store
            .exists(&a_declared_digest(&piece))
            .await
            .expect("the store answers"),
        "the sweep published unverified bytes"
    );
    // The authority was closed by the sweep itself.
    let record = journal
        .get(&id)
        .await
        .expect("the journal answers")
        .expect("the session stands closed");
    assert!(record.closed, "the swept session is still open");
}
