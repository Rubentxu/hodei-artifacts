//! The content store by its digest: the CAS.
//!
//! Starting with M9 Hodei **owns the bytes**, and the first property of a
//! store that owns them is that **the same content is not stored twice**:
//! the identity of the content is its digest, and the digest is what
//! is stored.
//!
//! And the second is that what comes out by the digest is **exactly** what
//! went in: on read, the digest is recomputed and compared. A store
//! that returns something without checking is not a content store, it is a
//! guess about what was there.
//!
//! # What changes with the production CAS
//!
//! The M9 CAS was correct in its central guarantee but not operable as the
//! basis of a product that receives client bytes:
//!
//! - **It wrote the temporary file with a name per digest.** Two producers that
//!   upload the same content at the same time wrote to the **same**
//!   temporary file, and the rename of one could publish the bytes
//!   of the other. Now each write in progress has its **own** staging
//!   file, and the commit is the only thing that publishes.
//! - **It buffered the whole content.** `put(&[u8])` already received the
//!   content in memory. Now there is a write session that accepts
//!   parts: process memory does not grow with the size of the artifact.
//! - **It did not validate what it published.** The commit checks the algorithm,
//!   length and digest against what the client declares, and if it does not
//!   match, the staging is removed and **no visible reference
//!   remains**.
//! - **It did not distinguish failures.** A disk error and a content that is not
//!   there both came out as `NotFound`. Now each failure has its error:
//!   what is not there, what is broken and what could not be read.
//! - **It did not know how to clean itself.** Writes left half done when
//!   a process crashes stay on disk forever. Now there is staging with
//!   expiry and an operation that cleans it.
//!
//! And what does **not** change is the guarantee: the same content is not stored
//! twice, and what comes out by the digest is what went in.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use hodei_evidence::{ContentDigest, DigestAlgorithm};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::reclaim::{
    BlobReclaim, ConditionalBlob, ConditionalOutcome, Reclaimed, StoredBlob, VersionMarker,
};

/// The directory, inside the store root, where writes in progress
/// live.
const STAGING_DIR: &str = "staging";

/// Why the content store could not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobError {
    /// The content is not there.
    NotFound,
    /// The content read does not match its digest.
    Corrupted {
        /// The digest that was requested.
        expected: ContentDigest,
        /// The digest of what was read.
        found: ContentDigest,
    },
    /// The content does not have the length that was expected.
    LengthMismatch {
        /// The length that was expected.
        expected: u64,
        /// The length the content has.
        found: u64,
    },
    /// The content is not the one that was declared.
    DigestMismatch {
        /// The digest that was declared.
        expected: ContentDigest,
        /// The digest the content has.
        found: ContentDigest,
    },
    /// The digest algorithm is not computed by this store.
    UnsupportedAlgorithm(String),
    /// The write session no longer accepts more operations.
    SessionClosed,
    /// The store is not available.
    Unavailable(String),
}

impl std::fmt::Display for BlobError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(formatter, "the content is not there"),
            Self::Corrupted { expected, found } => write!(
                formatter,
                "the content does not match its digest: {expected} was asked and {found} is there"
            ),
            Self::LengthMismatch { expected, found } => write!(
                formatter,
                "the content is {found} bytes long and {expected} were expected"
            ),
            Self::DigestMismatch { expected, found } => write!(
                formatter,
                "the content does not match what was declared: {expected} was declared and {found} is there"
            ),
            Self::UnsupportedAlgorithm(algorithm) => write!(
                formatter,
                "the store cannot compute digests with {algorithm}"
            ),
            Self::SessionClosed => {
                write!(formatter, "the write session is closed")
            }
            Self::Unavailable(reason) => {
                write!(formatter, "the store is not available: {reason}")
            }
        }
    }
}

/// How a read behaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPolicy {
    /// The digest is recomputed and compared with the requested one.
    ///
    /// It is the default policy, and the only one that can be used for
    /// client content: what comes out has to be what went in.
    Verify,
    /// It is read without recomputing, for already verified content.
    ///
    /// It saves the recomputation, and checks **nothing**: using it on
    /// unverified content is returning a guess.
    Trust,
}

/// What a commit expects to find.
///
/// A commit does not publish "what has come in": it publishes **exactly** what
/// the client declares. If anything does not match, nothing is published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitExpectation {
    /// The digest of the expected content.
    pub digest: ContentDigest,
    /// The length of the expected content, in bytes.
    pub length: u64,
}

impl CommitExpectation {
    /// The expectation for content already held in memory.
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            digest: ContentDigest::of_bytes(bytes),
            length: bytes.len() as u64,
        }
    }
}

/// A write in progress.
///
/// The bytes come in parts and the commit publishes the whole content or
/// publishes nothing: a half-written blob **never** remains.
#[async_trait::async_trait]
pub trait BlobWriteSession: Send {
    /// Adds a part to the content in progress.
    async fn append(&mut self, chunk: &[u8]) -> Result<(), BlobError>;

    /// Publishes the content if and only if it is exactly the expected one.
    ///
    /// If the algorithm, length or digest do not match, the staging
    /// is removed and the error says so: no visible reference remains.
    async fn commit(&mut self, expectation: CommitExpectation) -> Result<ContentDigest, BlobError>;

    /// Discards the content in progress.
    async fn abort(&mut self) -> Result<(), BlobError>;

    /// How many bytes have been written.
    fn bytes_written(&self) -> u64;

    /// The digest of what has been written.
    ///
    /// It is computed as the parts pass through, and it is the same digest that the
    /// commit is going to check: whoever consults it does not recompute it
    /// on their own, because two computations of the same content are two
    /// opportunities to disagree.
    fn the_digest(&self) -> ContentDigest;

    /// Where it is being written, if the store has that notion.
    ///
    /// It serves to check that two writes in progress do **not** share a
    /// file: it is what guarantees that two producers of the same
    /// content do not step on each other.
    fn the_staging_path(&self) -> Option<&Path>;

    /// The **persistable** identifier of this staging.
    ///
    /// A durable session registry keeps this handle and, when
    /// reopening the session in another process, hands it back to
    /// [`BlobStore::resume`] so that the store recovers the exact
    /// write: the file path in the file store, the `key`/`upload_id`
    /// pair of the multipart in the object store. `None` says
    /// that this store cannot reopen a write: its sessions
    /// die with the process, as always.
    fn the_staging_handle(&self) -> Option<String> {
        None
    }

    /// Whether the session digest covers **all** the bytes of the staging.
    ///
    /// A session reopened cold may not have the hasher of the
    /// content that is already in the store --this is the case of the resumed
    /// multipart, whose already uploaded parts do not pass through here again--.
    /// Whoever closes must not compare a partial digest against what was
    /// declared: the store checks the whole content on publishing,
    /// which is the usual promise --nothing that does not add up is published--.
    fn the_digest_covers_all(&self) -> bool {
        true
    }

    /// Marks the write as **durable**: its staging survives the
    /// Drop of the handle.
    ///
    /// A registry with a journal can be left without a local handle (cache
    /// evicted, replica changed) without that meaning that nobody
    /// abandoned the upload: what decides the fate of the staging is the
    /// journal --commit, explicit abort or sweep--, not the last Drop.
    fn mark_durable(&mut self) {}

    /// Releases the local handle **without deleting** the durable staging.
    ///
    /// It is the hand that changes replica: the registry leaves the store
    /// closed --and its advisory lock free for another replica to
    /// claim-- after each request, when nothing remains below
    /// the offset that the authority declared
    /// ([`BlobWriteSession::flush_durable`]). A handle released here is not
    /// an abandonment: the authority remains open and the resume picks up
    /// the write by its handle.
    fn detach(&mut self) -> Result<(), BlobError> {
        // By default there is no durable staging to release: closing is
        // closing, and the store's Drop does its usual cleanup.
        Ok(())
    }

    /// The bytes that are **already in** the store, not in the buffer of the
    /// handle.
    ///
    /// In a multipart, an uploaded part is durable and what remains in the
    /// buffer is not: if the process dies, the journal's authority can only
    /// declare what the store has. The registry uses this to
    /// know whether it **can release** the handle without losing bytes: only when
    /// what is durable reaches what was written. A journal advances up to here, never
    /// beyond.
    fn the_durable_offset(&self) -> u64 {
        self.bytes_written()
    }

    /// Leaves in the store what is already durable and returns that offset: what
    /// survives the death of this process.
    ///
    /// The journal can only advance up to here. In the file store
    /// it is the synced file --everything written--. In the
    /// multipart it is the parts **confirmed** by the server, with those
    /// in flight already collected: what remains in the buffer did not
    /// leave the process, and the client resends it against the `Range` that
    /// the authority declares.
    async fn flush_durable(&mut self) -> Result<u64, BlobError> {
        Ok(self.bytes_written())
    }
}

/// The content store.
#[async_trait::async_trait]
pub trait BlobStore: Send + Sync {
    /// Stores the content and returns its digest.
    ///
    /// Storing the same content twice is not an error nor does it store it twice:
    /// the digest is the identity, and what has that digest is already
    /// there.
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError>;

    /// Reads the content of a digest.
    ///
    /// What is returned has **the requested digest**, and the store
    /// checks it before returning it.
    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError>;

    /// Whether the content is there.
    ///
    /// This says whether the content **is there**, not whether it is **intact**: integrity
    /// is checked with `verify` or with a verifying read. A store
    /// that uses this to assert that its bytes are those its digest says
    /// is lying.
    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError>;

    /// Opens a streaming write session.
    ///
    /// The session is the way to upload content that does not fit in memory:
    /// the bytes come in in parts and the commit publishes the whole.
    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError>;

    /// Reopens a write in progress by the handle left by
    /// [`BlobWriteSession::the_staging_handle`].
    ///
    /// It is the path of a **durable** session registry: a session
    /// opened on one replica is resumed on another without losing the bytes
    /// that are already in the store. The handle is opaque to the
    /// caller --a path of the store's own, a
    /// `key`/`upload_id` pair-- and the `written` offset is what the journal
    /// declared: the store accepts it as truth **only if what is
    /// in the staging matches it**; if not, the resume fails and the
    /// registry aborts the session, never resuming it over doubtful
    /// bytes.
    ///
    /// The default says that this store does not reopen: a session of its own
    /// dies with the process.
    async fn resume(
        &self,
        _handle: &str,
        _written: u64,
    ) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        Err(BlobError::Unavailable(
            "this store cannot reopen an in-progress write".to_owned(),
        ))
    }

    /// Reads the content applying an explicit policy.
    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError>;

    /// The length of the content, without reading it.
    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError>;

    /// How long ago the content was last written or reused, from metadata only
    /// (HSP-326).
    ///
    /// `Ok(None)` means the store keeps no such date: it is "unknown", never
    /// "old". It reads nothing of the content and writes nothing.
    async fn age(&self, digest: &ContentDigest) -> Result<Option<Duration>, BlobError> {
        let _ = digest;
        Ok(None)
    }

    /// Reads a **range** of the content.
    ///
    /// The range starts at `offset` and has `length` bytes. It is the way
    /// to serve a part of a content without fetching the whole content:
    /// an HTTP `Range` cannot ask for one byte and force the store
    /// to read it all.
    ///
    /// If the content does not cover the range, the error is
    /// `LengthMismatch`: the store does not return a short range as if
    /// it were the requested one, because a client that asks for 100 bytes and receives 40
    /// cannot know that 60 are missing.
    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError>;

    /// Checks that the content is still the one its digest says.
    ///
    /// If the content is not there, the error is `NotFound`; if it is there but does not
    /// correspond to its digest, the error is `Corrupted`. They are distinct
    /// failures and are not confused.
    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError>;

    /// The byte-reclaim view, if the store has it.
    ///
    /// A store that cannot list or delete with a barrier cannot be
    /// collected: the collector refuses instead of guessing.
    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        None
    }

    /// The conditional-delete view, if the store has it (HSP-192,
    /// AC2).
    ///
    /// A store without a safe conditional capability can still be
    /// collected in legacy mode; what it cannot do is take part in a
    /// coordinated pass over the original key without versioned
    /// quarantine. The collector asks this, not a configuration
    /// flag.
    fn conditional(&self) -> Option<&dyn crate::reclaim::ConditionalBlob> {
        None
    }
}

/// The content store in a directory.
///
/// The blob lives at `<directory>/<ab>/<cd>/<digest>`: the first two
/// characters spread the files across directories, which is what makes
/// a directory with a million blobs not a problem for the file
/// system.
///
/// And the writes in progress live apart, in
/// `<directory>/staging/<id>.part`, with a name **of its own** for each
/// write: two producers of the same content never share a file.
pub struct FileBlobStore {
    root: PathBuf,
    /// The barrier between publishing a blob and collecting it.
    ///
    /// The commits that publish or reuse a blob, and the collector's
    /// delete, go through here: neither sees the other's work half done.
    gate: Arc<tokio::sync::Mutex<()>>,
}

impl FileBlobStore {
    /// The store over a directory.
    ///
    /// The directory is created if it is not there.
    pub fn new(root: impl AsRef<Path>) -> Result<Self, BlobError> {
        let root = root.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        Ok(Self {
            root,
            gate: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// Where a blob lives.
    pub fn the_path_of(&self, digest: &ContentDigest) -> PathBuf {
        blob_path(&self.root, digest)
    }

    /// The directory where the writes in progress live.
    pub fn the_staging_dir(&self) -> PathBuf {
        self.root.join(STAGING_DIR)
    }

    /// The writes in progress that exist right now.
    ///
    /// Only those that are **of this store**: staging is a convention
    /// of the directory, and a file with another name there is not a
    /// write in progress of ours. They are listed with the age they have, and a
    /// file whose date cannot be read is **not** listed: without a date there is no
    /// age, and what is not known is not touched.
    pub fn staging_entries(&self) -> Result<Vec<StagingEntry>, BlobError> {
        let mut entries = Vec::new();
        let read = match std::fs::read_dir(self.the_staging_dir()) {
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(entries),
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        for entry in read {
            let entry = entry.map_err(|error| BlobError::Unavailable(error.to_string()))?;
            let name = entry.file_name();
            if !is_a_staging_file(&name.to_string_lossy()) {
                continue;
            }
            let metadata = entry
                .metadata()
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
            let Some(modified) = metadata.modified().ok() else {
                // Without a date nothing can be decided: the file stays.
                continue;
            };
            let age = SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO);
            entries.push(StagingEntry {
                path: entry.path(),
                length: metadata.len(),
                age,
            });
        }
        Ok(entries)
    }

    /// Removes the writes in progress older than `max_age`.
    ///
    /// It is the recovery operation: a process that crashes halfway through
    /// an upload leaves its staging on disk, and that staging is never
    /// published. With expiry, it cleans itself up.
    ///
    /// What decides is not the age, but the **lock**: while a
    /// write lives, its file is locked by the process that
    /// writes, and a locked file is not deleted. That is why age is the
    /// margin of prudence and not the criterion: an old file from an
    /// upload that has gone hours without writing is still from a live
    /// upload, and pulling it out from under it breaks the upload.
    ///
    /// A file that could not be deleted is **not** a reason for the
    /// store not to serve: a warning is given and it carries on. What it cannot delete is a
    /// disk problem, not a reason not to start.
    pub fn clean_expired_staging(&self, max_age: Duration) -> Result<StagingCleanup, BlobError> {
        let mut cleanup = StagingCleanup::default();
        for entry in self.staging_entries()? {
            if entry.age < max_age {
                continue;
            }
            // The lock says whether someone is writing this file:
            // if so, it belongs to a live upload and is not touched.
            let file = match std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&entry.path)
            {
                Ok(file) => file,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => match std::fs::OpenOptions::new().read(true).open(&entry.path) {
                    Ok(file) => file,
                    Err(error) => {
                        cleanup
                            .failed
                            .push((entry.path.display().to_string(), error.to_string()));
                        continue;
                    }
                },
            };
            match file.try_lock() {
                Ok(()) => {}
                Err(std::fs::TryLockError::WouldBlock) => {
                    cleanup.alive += 1;
                    continue;
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    cleanup
                        .failed
                        .push((entry.path.display().to_string(), error.to_string()));
                    continue;
                }
            }
            match std::fs::remove_file(&entry.path) {
                Ok(()) => cleanup.removed += 1,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    cleanup
                        .failed
                        .push((entry.path.display().to_string(), error.to_string()));
                }
            }
        }
        Ok(cleanup)
    }
}

/// Whether the name is that of a write in progress of this store.
///
/// The pattern is set by `new_session_id`: `<pid>-<nanos:x>-<counter:x>.part`.
/// A file with another name in the staging directory is not of this
/// store: it is not listed, not deleted and not counted.
fn is_a_staging_file(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".part") else {
        return false;
    };
    let mut parts = stem.split('-');
    let (Some(pid), Some(nanos), Some(counter), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    !pid.is_empty()
        && pid.bytes().all(|byte| byte.is_ascii_digit())
        && !nanos.is_empty()
        && nanos.bytes().all(|byte| byte.is_ascii_hexdigit())
        && !counter.is_empty()
        && counter.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// What happened when cleaning the staging.
///
/// The cleanup does **not** fail: a file that could not be deleted is counted and
/// a warning is given, and the store keeps serving. What is needed to
/// count it is this.
///
/// Shared by both backends: for the file one the identifier
/// is the path; for the object one, the key. Both are displayed the same.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StagingCleanup {
    /// How many leftovers were removed.
    pub removed: usize,
    /// How many leftovers there were that are being written right now.
    ///
    /// They are from live uploads: they are not touched.
    pub alive: usize,
    /// The leftovers that could not be deleted, with their reason.
    pub failed: Vec<(String, String)>,
}

/// A write in progress in the file store.
///
/// The staging is deleted only if the session is abandoned without committing: the
/// `Drop` cleans it up, so that an error, a `?` or a panic leave no
/// garbage on disk.
pub struct FileBlobWriteSession {
    root: PathBuf,
    gate: Arc<tokio::sync::Mutex<()>>,
    staging_path: PathBuf,
    file: Option<tokio::fs::File>,
    hasher: Sha256,
    written: u64,
    open: bool,
    /// The registry journal declared the session durable: the handle's Drop
    /// does **not** touch the staging. Its fate is decided by the journal.
    durable: bool,
    /// Whether the local hasher covers the whole staging. A handle reopened
    /// cold has not seen again the bytes that were already in the file: its
    /// local digest is partial, and the commit checks the complete
    /// content before publishing.
    covers_all: bool,
}

impl FileBlobWriteSession {
    /// The staging file of this write.
    pub fn the_staging_path(&self) -> &Path {
        &self.staging_path
    }

    /// The handle over an already existing staging file, in append mode.
    ///
    /// The resume of a journal goes through here: the file lock
    /// --the `try_lock` of `begin`-- is claimed and the file is trimmed to the offset that the
    /// authority declared. Without the lock, the upload is still writing on
    /// another replica and this resume fails: there are not two writers of the same
    /// staging.
    ///
    /// A file **longer** than declared is not an error: these are the
    /// bytes that a writer wrote and did not manage to record before
    /// dying. They are trimmed --never published ahead of the
    /// authority--, and the upload continues from the declared offset. A
    /// shorter one would be: the authority cannot owe bytes to the disk.
    ///
    /// The hasher is NOT walked here: a resume can happen on every
    /// request and recomputing the whole file would be paying twice. The
    /// reopened session is marked as **partial** and the commit checks
    /// the digest by rereading the staging --the verification is the same, it is
    /// paid once, on publishing--.
    fn reopen(
        root: PathBuf,
        gate: Arc<tokio::sync::Mutex<()>>,
        staging_path: PathBuf,
        declared: u64,
    ) -> Result<Self, BlobError> {
        let file = std::fs::OpenOptions::new()
            .append(true)
            .open(&staging_path)
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        if let Err(error) = file.try_lock() {
            // The lock is the proof that the previous owner died:
            // a staging that another write has taken is NOT reopened.
            return Err(BlobError::Unavailable(match error {
                std::fs::TryLockError::WouldBlock => {
                    "the staging file is being written by another process".to_owned()
                }
                std::fs::TryLockError::Error(error) => error.to_string(),
            }));
        }
        let length = file
            .metadata()
            .map_err(|error| BlobError::Unavailable(error.to_string()))?
            .len();
        if length < declared {
            let _ = file.unlock();
            return Err(BlobError::Unavailable(format!(
                "the staging file holds {length} bytes and the journal declared {declared}"
            )));
        }
        if length > declared {
            // What the authority did not record is not published: it is trimmed
            // to the declared offset. These are the bytes of a dead writer
            // --it would not be alive, the lock prevents it-- that did not manage to
            // record its progress, or a duplicate that the CAS already burst.
            // The client resends them against the declared `Range`.
            file.set_len(declared)
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }
        // The hasher is not recomputed here: a resume can happen on every
        // request and rereading the whole file would be paying twice.
        // The reopened session is marked as **partial** --`covers_all`
        // below-- and the commit checks the digest against the complete
        // staging, which is the same honest verification, only once,
        // on publishing.
        let file = tokio::fs::File::from_std(file);
        Ok(Self {
            root,
            gate,
            staging_path,
            file: Some(file),
            hasher: Sha256::new(),
            written: declared,
            open: true,
            durable: true,
            covers_all: false,
        })
    }

    /// Closes the session and deletes the staging, publishing nothing.
    fn discard(&mut self) {
        self.open = false;
        self.file = None;
        let _ = std::fs::remove_file(&self.staging_path);
    }
}

impl Drop for FileBlobWriteSession {
    fn drop(&mut self) {
        if self.open && !self.durable {
            self.discard();
        }
    }
}

#[async_trait::async_trait]
impl BlobWriteSession for FileBlobWriteSession {
    async fn append(&mut self, chunk: &[u8]) -> Result<(), BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        let file = self.file.as_mut().ok_or(BlobError::SessionClosed)?;
        file.write_all(chunk)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        self.hasher.update(chunk);
        self.written += chunk.len() as u64;
        Ok(())
    }

    async fn commit(&mut self, expectation: CommitExpectation) -> Result<ContentDigest, BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        // The algorithm has to be one this store knows how to compute:
        // otherwise what has been written cannot even be verified.
        if expectation.digest.algorithm != DigestAlgorithm::Sha256 {
            self.discard();
            return Err(BlobError::UnsupportedAlgorithm(
                expectation.digest.algorithm.to_string(),
            ));
        }

        // What is published is on disk, not in the system buffer.
        {
            let file = self.file.as_mut().ok_or(BlobError::SessionClosed)?;
            file.flush()
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
            file.sync_all()
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }

        // The honest digest check. When the handle has passed through
        // all the bytes, the local hasher is the truth and there is no need to
        // read again. When the session was reopened cold --another
        // process wrote the first bytes--, the local hasher does NOT
        // cover them: comparing that partial digest against what was declared would give
        // a false `DigestMismatch` and abort a good upload. It is
        // recomputed over the complete staging, only once, on publishing.
        let found = if self.covers_all {
            ContentDigest::sha256(hex_of(&self.hasher.finalize_reset()))
        } else {
            let bytes = tokio::fs::read(&self.staging_path)
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
            ContentDigest::of_bytes(&bytes)
        };

        // The length is checked against the declared one.
        if self.written != expectation.length {
            self.discard();
            return Err(BlobError::LengthMismatch {
                expected: expectation.length,
                found: self.written,
            });
        }
        // And the digest, against the declared one.
        if found != expectation.digest {
            self.discard();
            return Err(BlobError::DigestMismatch {
                expected: expectation.digest,
                found,
            });
        }

        let target = blob_path(&self.root, &found);
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }

        // From here until publishing, the collector cannot delete anything:
        // what is looked at (the blob is there) remains true when acting.
        let gate = Arc::clone(&self.gate);
        let _barrier = gate.lock().await;

        // The same content is not stored twice: if the blob is already there,
        // the staging is thrown away and the same digest is returned. And its
        // date is renewed: reusing a blob is what tells the collector
        // that it is not an orphan even if the catalog does not see it yet.
        if target.exists() {
            touch(&target)?;
            self.discard();
            return Ok(found);
        }

        // The rename is atomic: a reader sees the whole blob or sees
        // nothing. It never sees half-written bytes.
        tokio::fs::rename(&self.staging_path, &target)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        // The file keeps the date of its last part, which may be
        // earlier than the grace if the session was stopped: the publication
        // date is now's.
        touch(&target)?;
        // And the directory is synced, so that the rename survives
        // a power cut.
        if let Some(parent) = target.parent()
            && let Ok(directory) = tokio::fs::File::open(parent).await
        {
            let _ = directory.sync_all().await;
        }

        self.open = false;
        self.file = None;
        Ok(found)
    }

    async fn abort(&mut self) -> Result<(), BlobError> {
        if !self.open {
            return Ok(());
        }
        let outcome = std::fs::remove_file(&self.staging_path);
        self.open = false;
        self.file = None;
        match outcome {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    fn bytes_written(&self) -> u64 {
        self.written
    }

    fn the_digest(&self) -> ContentDigest {
        // The hasher is cloned so as not to consume it: the session can keep
        // writing, and the digest of what has been written so far has to
        // remain the same after the next part.
        ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()))
    }

    fn the_staging_path(&self) -> Option<&Path> {
        Some(&self.staging_path)
    }

    /// The persistable handle: the path of the staging file.
    ///
    /// Another process sharing the disk can claim the exact write
    /// by this path, and the file mark --the `try_lock`--
    /// says whether the previous owner is still alive: there are not two writers of
    /// the same staging.
    fn the_staging_handle(&self) -> Option<String> {
        Some(self.staging_path.display().to_string())
    }

    /// The journal takes ownership of the staging: the handle's Drop no longer
    /// deletes it, its life cycle is decided by the journal.
    fn mark_durable(&mut self) {
        self.durable = true;
    }

    /// Releases the file **without deleting it**: closing the descriptor releases the
    /// advisory lock, and the journal's authority decides when the
    /// staging ceased to have an owner. It is the hand that changes replica.
    fn detach(&mut self) -> Result<(), BlobError> {
        self.open = false;
        self.file = None;
        Ok(())
    }

    /// A handle reopened cold has not again passed the first bytes
    /// through the hasher: its local digest is partial and the commit checks
    /// the complete file.
    fn the_digest_covers_all(&self) -> bool {
        self.covers_all
    }

    /// The staging file is durable byte by byte: releasing the handle --or
    /// the process dying-- loses no bytes, and the authority can
    /// declare everything written. What remains below is the system
    /// page: it is forced before declaring.
    async fn flush_durable(&mut self) -> Result<u64, BlobError> {
        let file = self.file.as_mut().ok_or(BlobError::SessionClosed)?;
        file.flush()
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        file.sync_all()
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        Ok(self.written)
    }
}

#[async_trait::async_trait]
impl BlobStore for FileBlobStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        let length = bytes.len() as u64;

        // The same content is not stored twice. If the blob is already there,
        // at least it is checked that it is not a truncated leftover: the
        // complete verification of the stored content is a task for the
        // scrub, not for every write.
        let gate = Arc::clone(&self.gate);
        let barrier = gate.lock().await;
        if self.exists(&digest).await.unwrap_or(false) {
            match self.length(&digest).await {
                Ok(stored) if stored == length => {
                    touch(&self.the_path_of(&digest))?;
                    return Ok(digest);
                }
                Ok(stored) => {
                    return Err(BlobError::LengthMismatch {
                        expected: length,
                        found: stored,
                    });
                }
                Err(_) => { /* se sigue por el camino normal */ }
            }
        }

        // The barrier is released before writing: the commit takes it on
        // its own, and writing the bytes does not need to exclude anyone.
        drop(barrier);
        let mut session = self.begin().await?;
        session.append(bytes).await?;
        session
            .commit(CommitExpectation {
                digest: digest.clone(),
                length,
            })
            .await
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await
    }

    fn reclaimer(&self) -> Option<&dyn BlobReclaim> {
        Some(self)
    }

    fn conditional(&self) -> Option<&dyn ConditionalBlob> {
        Some(self)
    }

    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        match tokio::fs::metadata(self.the_path_of(digest)).await {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        let staging_dir = self.the_staging_dir();
        tokio::fs::create_dir_all(&staging_dir)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        // The name is **its own** for this write: two producers of the
        // same content write to different files.
        let staging_path = staging_dir.join(format!("{}.part", new_session_id()));
        let file = tokio::fs::File::create(&staging_path)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        // The file is locked while it is written: the lock is what
        // says that this write is **alive**, and what prevents another
        // process's cleanup from taking it from underneath. Without this, a
        // startup that finds the old file takes it and the upload
        // that had been writing for minutes fails on publishing.
        let std_file = match file.try_into_std() {
            Ok(std_file) => std_file,
            Err(file) => file.into_std().await,
        };
        std_file
            .try_lock()
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        let file = tokio::fs::File::from_std(std_file);
        Ok(Box::new(FileBlobWriteSession {
            root: self.root.clone(),
            gate: Arc::clone(&self.gate),
            staging_path,
            file: Some(file),
            hasher: Sha256::new(),
            written: 0,
            open: true,
            durable: false,
            covers_all: true,
        }))
    }

    /// Reopens a write in progress from the handle that a
    /// journal stored: the path of the staging file.
    ///
    /// Only one replica can have the file taken --the advisory
    /// lock of `begin`--. A resume that arrives after the death
    /// of the previous owner reclaims the lock and keeps writing where
    /// it left off; one that arrives while the owner lives, fails.
    async fn resume(
        &self,
        handle: &str,
        written: u64,
    ) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        let path = std::path::Path::new(handle);
        // The handle must be a staging of this store, not an
        // arbitrary path: everything that does not hang from the
        // store's own staging directory is rejected.
        let staging_dir = tokio::fs::canonicalize(self.the_staging_dir())
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        let canonical = tokio::fs::canonicalize(path)
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        if !canonical.starts_with(&staging_dir)
            || canonical.extension() != Some(std::ffi::OsStr::new("part"))
        {
            return Err(BlobError::Unavailable(
                "the session handle is not a staging file of this store".to_owned(),
            ));
        }
        let session = FileBlobWriteSession::reopen(
            self.root.clone(),
            Arc::clone(&self.gate),
            canonical,
            written,
        )?;
        Ok(Box::new(session))
    }

    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        let bytes = match tokio::fs::read(self.the_path_of(digest)).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(BlobError::NotFound);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        if policy == ReadPolicy::Verify {
            let found = ContentDigest::of_bytes(&bytes);
            if found != *digest {
                return Err(BlobError::Corrupted {
                    expected: digest.clone(),
                    found,
                });
            }
        }
        Ok(bytes)
    }

    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        match tokio::fs::metadata(self.the_path_of(digest)).await {
            Ok(metadata) => Ok(metadata.len()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(BlobError::NotFound),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    async fn age(&self, digest: &ContentDigest) -> Result<Option<Duration>, BlobError> {
        let unavailable = |error: std::io::Error| BlobError::Unavailable(error.to_string());
        match tokio::fs::metadata(self.the_path_of(digest)).await {
            Ok(metadata) => {
                let modified = metadata.modified().map_err(unavailable)?;
                Ok(Some(
                    SystemTime::now()
                        .duration_since(modified)
                        .unwrap_or(Duration::ZERO),
                ))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(BlobError::NotFound),
            Err(error) => Err(unavailable(error)),
        }
    }

    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        use tokio::io::{AsyncReadExt, AsyncSeekExt};
        let path = self.the_path_of(digest);
        let mut file = match tokio::fs::File::open(&path).await {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(BlobError::NotFound);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        // The range is requested in place: what is not wanted is skipped and
        // only the requested bytes are read. A store that read the whole file
        // to return 100 bytes of a gigabyte would be
        // obeying the client at the expense of everyone else.
        if offset > 0 {
            file.seek(std::io::SeekFrom::Start(offset))
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }
        // The buffer is reserved with what **there is**, not with what the
        // caller asks for: a range that does not fit in the file is not reserved,
        // it is said not to exist. Asking for a range of a gigabyte from a
        // one-kilobyte file cannot bring down the process over a
        // `vec![0; length]`.
        let size = file
            .metadata()
            .await
            .map_err(|error| BlobError::Unavailable(error.to_string()))?
            .len();
        let available = size.saturating_sub(offset);
        if offset > size || length > available {
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: available,
            });
        }
        let mut bytes = vec![0u8; usize::try_from(length).unwrap_or(0)];
        // It is read until the buffer is full or the end of the file: a
        // single read may return less than requested without anything missing,
        // and a half range would be a range that is not the one
        // requested.
        let mut read = 0usize;
        while read < bytes.len() {
            let chunk = file
                .read(&mut bytes[read..])
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
            if chunk == 0 {
                break;
            }
            read += chunk;
        }
        if read as u64 != length {
            // The content is shorter than requested: the range does not exist.
            // It is said how much was read, so that whoever asked knows that
            // bytes are missing.
            return Err(BlobError::LengthMismatch {
                expected: length,
                found: read as u64,
            });
        }
        Ok(bytes)
    }

    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await.map(|_| ())
    }
}

/// The in-memory content store.
///
/// It is the one used in tests and fixture mode; the semantics are the
/// same as those of the on-disk store, including the digest check
/// on read.
///
/// The difference is that it **keeps the bytes in memory**, and that is
/// inherent to what it is: it serves for fixtures and tests, not for client
/// artifacts.
#[derive(Debug, Clone, Default)]
pub struct InMemoryBlobStore {
    blobs: Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>>,
}

impl InMemoryBlobStore {
    /// The store, empty.
    pub fn empty() -> Self {
        Self::default()
    }
}

/// A write in progress in the in-memory store.
///
/// It accumulates the bytes because that is what an in-memory store does; the
/// integrity guarantee is the same.
///
/// And the commit **stores**: the content enters the store with the same
/// digest as that of the on-disk store. A session that validates and does not store
/// would be a digest of nothing.
pub struct InMemoryBlobWriteSession {
    blobs: Arc<std::sync::Mutex<std::collections::BTreeMap<String, Vec<u8>>>>,
    bytes: Vec<u8>,
    hasher: Sha256,
    open: bool,
}

#[async_trait::async_trait]
impl BlobWriteSession for InMemoryBlobWriteSession {
    async fn append(&mut self, chunk: &[u8]) -> Result<(), BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        self.bytes.extend_from_slice(chunk);
        self.hasher.update(chunk);
        Ok(())
    }

    async fn commit(&mut self, expectation: CommitExpectation) -> Result<ContentDigest, BlobError> {
        if !self.open {
            return Err(BlobError::SessionClosed);
        }
        self.open = false;
        if expectation.digest.algorithm != DigestAlgorithm::Sha256 {
            return Err(BlobError::UnsupportedAlgorithm(
                expectation.digest.algorithm.to_string(),
            ));
        }
        let length = self.bytes.len() as u64;
        if length != expectation.length {
            return Err(BlobError::LengthMismatch {
                expected: expectation.length,
                found: length,
            });
        }
        let found = ContentDigest::sha256(hex_of(&self.hasher.finalize_reset()));
        if found != expectation.digest {
            return Err(BlobError::DigestMismatch {
                expected: expectation.digest,
                found,
            });
        }
        // And what has been validated is stored: the same content, with the
        // same digest, stored only once.
        let mut blobs = self.blobs.lock().expect("el almacén no está envenenado");
        blobs.insert(found.value.clone(), std::mem::take(&mut self.bytes));
        Ok(found)
    }

    async fn abort(&mut self) -> Result<(), BlobError> {
        self.open = false;
        self.bytes.clear();
        Ok(())
    }

    fn bytes_written(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn the_digest(&self) -> ContentDigest {
        ContentDigest::sha256(hex_of(&self.hasher.clone().finalize()))
    }

    fn the_staging_path(&self) -> Option<&Path> {
        None
    }
}

#[async_trait::async_trait]
impl BlobStore for InMemoryBlobStore {
    async fn put(&self, bytes: &[u8]) -> Result<ContentDigest, BlobError> {
        let digest = ContentDigest::of_bytes(bytes);
        let mut blobs = self.blobs.lock().expect("el almacén no está envenenado");
        // The same content is not stored twice: the key is the
        // digest.
        blobs.insert(digest.value.clone(), bytes.to_vec());
        Ok(digest)
    }

    async fn get(&self, digest: &ContentDigest) -> Result<Vec<u8>, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        let bytes = blobs
            .get(&digest.value.clone())
            .ok_or(BlobError::NotFound)?
            .clone();
        let found = ContentDigest::of_bytes(&bytes);
        if found != *digest {
            return Err(BlobError::Corrupted {
                expected: digest.clone(),
                found,
            });
        }
        Ok(bytes)
    }

    async fn exists(&self, digest: &ContentDigest) -> Result<bool, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        Ok(blobs.contains_key(&digest.value.clone()))
    }

    async fn begin(&self) -> Result<Box<dyn BlobWriteSession>, BlobError> {
        Ok(Box::new(InMemoryBlobWriteSession {
            blobs: Arc::clone(&self.blobs),
            bytes: Vec::new(),
            hasher: Sha256::new(),
            open: true,
        }))
    }

    async fn get_with(
        &self,
        digest: &ContentDigest,
        policy: ReadPolicy,
    ) -> Result<Vec<u8>, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        let bytes = blobs
            .get(&digest.value.clone())
            .ok_or(BlobError::NotFound)?
            .clone();
        if policy == ReadPolicy::Verify {
            let found = ContentDigest::of_bytes(&bytes);
            if found != *digest {
                return Err(BlobError::Corrupted {
                    expected: digest.clone(),
                    found,
                });
            }
        }
        Ok(bytes)
    }

    async fn length(&self, digest: &ContentDigest) -> Result<u64, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        let bytes = blobs
            .get(&digest.value.clone())
            .ok_or(BlobError::NotFound)?;
        Ok(bytes.len() as u64)
    }

    async fn read_range(
        &self,
        digest: &ContentDigest,
        offset: u64,
        length: u64,
    ) -> Result<Vec<u8>, BlobError> {
        let blobs = self.blobs.lock().expect("el almacén no está envenenado");
        let bytes = blobs
            .get(&digest.value.clone())
            .ok_or(BlobError::NotFound)?;
        let start = usize::try_from(offset).unwrap_or(usize::MAX);
        let end = start.saturating_add(usize::try_from(length).unwrap_or(usize::MAX));
        if start > bytes.len() || end > bytes.len() {
            // The content is shorter than requested: the range does not exist.
            let found = bytes.len().saturating_sub(start) as u64;
            return Err(BlobError::LengthMismatch {
                expected: length,
                found,
            });
        }
        Ok(bytes[start..end].to_vec())
    }

    async fn verify(&self, digest: &ContentDigest) -> Result<(), BlobError> {
        self.get_with(digest, ReadPolicy::Verify).await.map(|_| ())
    }
}

/// A write in progress that has been left on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagingEntry {
    /// Where the staging file is.
    pub path: PathBuf,
    /// How many bytes it has written.
    pub length: u64,
    /// How long it has been written.
    pub age: Duration,
}

/// Where a blob lives, within a root.
fn blob_path(root: &Path, digest: &ContentDigest) -> PathBuf {
    let hex = &digest.value;
    root.join(&hex[..2]).join(&hex[2..4]).join(hex)
}

/// A unique identifier for a write in progress.
///
/// It combines the process, the clock and a counter, so that two writes
/// **of the same** content, and even from different processes, never
/// share a staging file.
fn new_session_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos:x}-{counter:x}", std::process::id())
}

/// The hexadecimal of the bytes.
fn hex_of(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Renews the date of a blob: "it was used now".
///
/// It is the mark that the collector reads. If it cannot be written, the error
/// propagates: reusing a blob without leaving the mark is leaving the collector
/// free to delete it.
fn touch(path: &Path) -> Result<(), BlobError> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .map_err(|error| BlobError::Unavailable(error.to_string()))?;
    file.set_modified(SystemTime::now())
        .map_err(|error| BlobError::Unavailable(error.to_string()))
}

/// The digest that the path of a blob says, if the path is that of one.
///
/// A blob is `<root>/<ab>/<cd>/<64 hex>`, with `ab` and `cd` the two pairs
/// of initial characters of the name. Anything else is not of this
/// store and is not listed.
fn digest_of_blob_file(prefix_a: &str, prefix_b: &str, name: &str) -> Option<ContentDigest> {
    let valid = name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !valid || &name[..2] != prefix_a || &name[2..4] != prefix_b {
        return None;
    }
    Some(ContentDigest::sha256(name))
}

#[async_trait::async_trait]
impl BlobReclaim for FileBlobStore {
    async fn list_blobs(&self) -> Result<Vec<StoredBlob>, BlobError> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let unavailable = |error: std::io::Error| BlobError::Unavailable(error.to_string());
            let mut blobs = Vec::new();
            for first in std::fs::read_dir(&root).map_err(unavailable)? {
                let first = first.map_err(unavailable)?;
                let first_name = first.file_name().to_string_lossy().into_owned();
                if first_name.len() != 2 || !first.file_type().map_err(unavailable)?.is_dir() {
                    continue;
                }
                for second in std::fs::read_dir(first.path()).map_err(unavailable)? {
                    let second = second.map_err(unavailable)?;
                    let second_name = second.file_name().to_string_lossy().into_owned();
                    if second_name.len() != 2 || !second.file_type().map_err(unavailable)?.is_dir()
                    {
                        continue;
                    }
                    for file in std::fs::read_dir(second.path()).map_err(unavailable)? {
                        let file = file.map_err(unavailable)?;
                        let name = file.file_name().to_string_lossy().into_owned();
                        // First the shape of the name: a quarantine
                        // object (HSP-192) shares the shard but carries a
                        // suffix, and what is not an exact digest is not
                        // of this store: it is neither listed nor deleted.
                        if name.len() != 64
                            || !name
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        {
                            continue;
                        }
                        let Some(digest) = digest_of_blob_file(&first_name, &second_name, &name)
                        else {
                            continue;
                        };
                        let metadata = match file.metadata() {
                            Ok(metadata) if metadata.is_file() => metadata,
                            Ok(_) => continue,
                            // It was deleted while it was being listed: it is no longer there.
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                                continue;
                            }
                            Err(error) => return Err(unavailable(error)),
                        };
                        // Without a date there is no age, and without age nothing can be
                        // decided: it is a failure, not a young blob.
                        let modified = metadata.modified().map_err(unavailable)?;
                        blobs.push(StoredBlob {
                            digest,
                            length: metadata.len(),
                            age: SystemTime::now()
                                .duration_since(modified)
                                .unwrap_or(Duration::ZERO),
                        });
                    }
                }
            }
            Ok(blobs)
        })
        .await
        .map_err(|error| BlobError::Unavailable(error.to_string()))?
    }

    async fn delete_if_older_than(
        &self,
        digest: &ContentDigest,
        grace: Duration,
    ) -> Result<Reclaimed, BlobError> {
        if digest.algorithm != DigestAlgorithm::Sha256 || digest.value.len() != 64 {
            return Err(BlobError::UnsupportedAlgorithm(
                digest.algorithm.to_string(),
            ));
        }
        let path = self.the_path_of(digest);
        let _barrier = self.gate.lock().await;
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Reclaimed::Gone);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        let modified = metadata
            .modified()
            .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or(Duration::ZERO);
        if age < grace {
            return Ok(Reclaimed::Fresh);
        }
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                // The delete is synced with the directory: a power
                // cut does not resurrect a blob that was already considered deleted.
                if let Some(parent) = path.parent()
                    && let Ok(directory) = tokio::fs::File::open(parent).await
                {
                    let _ = directory.sync_all().await;
                }
                Ok(Reclaimed::Deleted {
                    length: metadata.len(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Reclaimed::Gone),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }
}

/// The mark of a file version: the mtime in nanos and the size.
///
/// Two concurrent writes of different content produce different
/// marks; the rename does not touch the mark, so a quarantine or a
/// conditional delete under the barrier is atomic-safe.
fn file_marker(path_key: &str, metadata: &std::fs::Metadata) -> VersionMarker {
    let modified = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_nanos();
    VersionMarker {
        key: path_key.to_string(),
        marker: format!("mtime:{modified}:len:{}", metadata.len()),
        length: metadata.len(),
    }
}

#[async_trait::async_trait]
impl crate::reclaim::ConditionalBlob for FileBlobStore {
    fn supports_conditional_delete(&self) -> bool {
        // The rename under the store barrier is atomic and does not touch
        // the mark: on a local POSIX an exact version can be deleted
        // without trampling another's rewrite.
        true
    }

    fn quarantine_key_of(&self, digest: &ContentDigest, run_id: &str) -> String {
        self.root
            .join("quarantine")
            .join(run_id)
            .join(&digest.value)
            .to_string_lossy()
            .into_owned()
    }

    async fn head_marker(
        &self,
        digest: &ContentDigest,
    ) -> Result<Option<VersionMarker>, BlobError> {
        let path = self.the_path_of(digest);
        let key = path.to_string_lossy().into_owned();
        let _barrier = self.gate.lock().await;
        match tokio::fs::metadata(&path).await {
            Ok(metadata) => Ok(Some(file_marker(&key, &metadata))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    async fn delete_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
    ) -> Result<ConditionalOutcome, BlobError> {
        if digest.algorithm != DigestAlgorithm::Sha256 || digest.value.len() != 64 {
            return Err(BlobError::UnsupportedAlgorithm(
                digest.algorithm.to_string(),
            ));
        }
        let path = self.the_path_of(digest);
        let _barrier = self.gate.lock().await;
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConditionalOutcome::Gone);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        // The condition is looked at with the barrier taken: it remains
        // true when deleting underneath.
        let current = file_marker(&path.to_string_lossy(), &metadata);
        if current.marker != marker.marker {
            return Ok(ConditionalOutcome::NotApplied);
        }
        match tokio::fs::remove_file(&path).await {
            Ok(()) => {
                if let Some(parent) = path.parent()
                    && let Ok(directory) = tokio::fs::File::open(parent).await
                {
                    let _ = directory.sync_all().await;
                }
                Ok(ConditionalOutcome::Applied {
                    length: metadata.len(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConditionalOutcome::Gone)
            }
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }

    async fn quarantine_if_untouched(
        &self,
        digest: &ContentDigest,
        marker: &VersionMarker,
        quarantine_key: &str,
    ) -> Result<ConditionalOutcome, BlobError> {
        if digest.algorithm != DigestAlgorithm::Sha256 || digest.value.len() != 64 {
            return Err(BlobError::UnsupportedAlgorithm(
                digest.algorithm.to_string(),
            ));
        }
        let path = self.the_path_of(digest);
        let quarantine = std::path::Path::new(quarantine_key);
        let _barrier = self.gate.lock().await;
        let metadata = match tokio::fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConditionalOutcome::Gone);
            }
            Err(error) => return Err(BlobError::Unavailable(error.to_string())),
        };
        let current = file_marker(&path.to_string_lossy(), &metadata);
        if current.marker != marker.marker {
            return Ok(ConditionalOutcome::NotApplied);
        }
        // The quarantine is a NEW name, never the original key:
        // a rename under the barrier leaves the marked version readable
        // under the new name and frees the key for the publisher's
        // rewrite without a possible race.
        if let Some(parent) = quarantine.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|error| BlobError::Unavailable(error.to_string()))?;
        }
        match tokio::fs::rename(&path, quarantine).await {
            Ok(()) => Ok(ConditionalOutcome::Applied {
                length: metadata.len(),
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ConditionalOutcome::Gone)
            }
            Err(error) => Err(BlobError::Unavailable(error.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The content of the test: distinguishable bytes, so that a wrong
    /// range is noticed.
    fn the_bytes() -> Vec<u8> {
        (0u8..=255).collect()
    }

    #[tokio::test]
    async fn the_range_read_returns_exactly_the_bytes_asked_for() {
        let bytes = the_bytes();
        let digest = ContentDigest::of_bytes(&bytes);
        let store = InMemoryBlobStore::empty();
        store.put(&bytes).await.expect("the content is stored");

        let range = store
            .read_range(&digest, 10, 5)
            .await
            .expect("the range is readable");
        assert_eq!(range, bytes[10..15]);
    }

    #[tokio::test]
    async fn the_range_read_says_when_the_content_is_shorter() {
        let bytes = the_bytes();
        let digest = ContentDigest::of_bytes(&bytes);
        let store = InMemoryBlobStore::empty();
        store.put(&bytes).await.expect("the content is stored");

        // A range that goes past the end is not a short range: it is a
        // range that does not exist, and the error says so.
        let error = store
            .read_range(&digest, 250, 10)
            .await
            .expect_err("the range does not exist");
        assert_eq!(
            error,
            BlobError::LengthMismatch {
                expected: 10,
                found: 6
            }
        );
    }

    #[tokio::test]
    async fn the_range_read_of_a_content_that_is_not_there_is_not_found() {
        let store = InMemoryBlobStore::empty();
        let digest = ContentDigest::of_bytes(b"nothing");
        let error = store
            .read_range(&digest, 0, 1)
            .await
            .expect_err("the content is not there");
        assert_eq!(error, BlobError::NotFound);
    }

    #[tokio::test]
    async fn the_range_read_of_the_file_store_matches_the_memory_one() {
        let bytes = the_bytes();
        let digest = ContentDigest::of_bytes(&bytes);
        let directory = tempfile::tempdir().expect("a temp dir");
        let store = FileBlobStore::new(directory.path()).expect("the store is usable");
        store.put(&bytes).await.expect("the content is stored");

        let range = store
            .read_range(&digest, 10, 5)
            .await
            .expect("the range is readable");
        assert_eq!(range, bytes[10..15]);

        let error = store
            .read_range(&digest, 250, 10)
            .await
            .expect_err("the range does not exist");
        assert_eq!(
            error,
            BlobError::LengthMismatch {
                expected: 10,
                found: 6
            }
        );
    }

    /// HSP-192, AC2: the version mark protects the one that was not seen.
    #[tokio::test]
    async fn a_conditional_delete_never_touches_an_unmarked_version() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let store = FileBlobStore::new(directory.path()).expect("the store is usable");
        let bytes = b"orphan bytes".to_vec();
        let digest = ContentDigest::of_bytes(&bytes);
        store.put(&bytes).await.expect("the content is stored");

        let marker = store
            .conditional()
            .expect("the file store is conditional")
            .head_marker(&digest)
            .await
            .expect("head works")
            .expect("the blob is there");

        // Another version (the one a publisher would put after rewriting):
        // the condition is not met and NOTHING is touched.
        let stale = VersionMarker {
            marker: format!("{}-otro", marker.marker),
            ..marker.clone()
        };
        let outcome = store
            .conditional()
            .unwrap()
            .delete_if_untouched(&digest, &stale)
            .await
            .expect("the conditional delete answers");
        assert_eq!(outcome, ConditionalOutcome::NotApplied);
        assert!(
            store.exists(&digest).await.expect("exists"),
            "an unmarked version is never deleted"
        );

        // The marked version does get deleted.
        let outcome = store
            .conditional()
            .unwrap()
            .delete_if_untouched(&digest, &marker)
            .await
            .expect("the conditional delete answers");
        assert!(
            matches!(outcome, ConditionalOutcome::Applied { length } if length == 12),
            "the marked version is deleted: {outcome:?}"
        );
        assert!(!store.exists(&digest).await.expect("exists"));
    }

    /// HSP-192, AC2: the quarantine is versioned --new name, never a
    /// rewrite of the original key-- and leaves the bytes reconcilable.
    #[tokio::test]
    async fn a_quarantine_moves_the_marked_version_to_a_new_name() {
        let directory = tempfile::tempdir().expect("a temp dir");
        let store = FileBlobStore::new(directory.path()).expect("the store is usable");
        let bytes = b"marked version".to_vec();
        let digest = ContentDigest::of_bytes(&bytes);
        store.put(&bytes).await.expect("the content is stored");

        let marker = store
            .conditional()
            .unwrap()
            .head_marker(&digest)
            .await
            .expect("head works")
            .expect("the blob is there");
        let quarantine_key = directory
            .path()
            .join("quarantine")
            .join(format!("{}.gc-run-1", digest.value))
            .to_string_lossy()
            .into_owned();

        let outcome = store
            .conditional()
            .unwrap()
            .quarantine_if_untouched(&digest, &marker, &quarantine_key)
            .await
            .expect("the quarantine answers");
        assert!(
            matches!(outcome, ConditionalOutcome::Applied { length } if length == 14),
            "the marked version is quarantined: {outcome:?}"
        );

        // The original key was freed; the bytes live under the new
        // name, intact.
        assert!(!store.exists(&digest).await.expect("exists"));
        let moved = std::fs::read(&quarantine_key).expect("the quarantined bytes are there");
        assert_eq!(moved, bytes);

        // And list_blobs does not see them: what it does not recognize as its own is not
        // listed nor deleted. The quarantine belongs to reconciliation, not
        // to the census.
        let listed = store
            .reclaimer()
            .unwrap()
            .list_blobs()
            .await
            .expect("the census works");
        assert!(
            listed.iter().all(|blob| blob.digest != digest),
            "a quarantined name is not a live blob"
        );
    }
}
