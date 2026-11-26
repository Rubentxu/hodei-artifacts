//! Upload sessions: the state that lives between requests.
//!
//! The upload protocol is **chunked**: a `POST` opens the session, one
//! or more `PATCH`es fill it and a `PUT` closes it. Between one request
//! and the next there is state that has to be remembered: how many bytes have come in, which
//! CAS write session is holding them, which account and which
//! repository the upload belongs to.
//!
//! **Without a journal**, that state lives here, in memory, and for three reasons:
//!
//! - **The identifier is opaque and cannot be guessed.** It is a 128-bit
//!   UUID, and a session that could be guessed could be written by
//!   anyone.
//! - **The account and the repository are part of the session's
//!   identity.** A session of another account is not "your session with another
//!   identifier": it is a session that does not exist, and is answered as
//!   such.
//! - **The session survives the request, not the process.** If the process
//!   dies, the session is lost and the client receives an explicit error
//!   (`BLOB_UPLOAD_UNKNOWN`), not an upload left hanging nor
//!   half-written content. The staging file it left is cleaned up at
//!   startup, like any abandoned staging.
//!
//! **With a durable journal** (HSP-201) the map becomes a **cache**: the
//! offset authority lives outside the process, in
//! [`SessionJournal`](crate::oci::session_journal::SessionJournal), and with
//! it:
//!
//! - The offset advance goes through a **revision CAS**: two writers of
//!   the same session do not both advance —the one that loses receives the real
//!   offset and the protocol answers the `416` with `Range`, which is what
//!   makes resending a chunk duplicate no part—.
//! - The session is **revived on any replica** against the CAS staging
//!   handle: the file path, or the multipart `key`/`upload_id`
//!   pair. The store's handle is a **reclaimable resource**,
//!   not a property of the process: each write lasts as long as a
//!   request and is released, and the next chunk resumes it here or on
//!   another replica. Physical exclusion is provided by the store —the file's
//!   advisory lock—.
//! - A handle that dies with its process **does not delete** a staging that the
//!   authority still has open: its fate is decided by the journal, the
//!   commit, the explicit abort and the sweep ([`UploadRegistry::sweep`]).
//!
//! What is **not** stored here is the content: the bytes go to the CAS through
//! its write session, and each upload's staging is its own
//! file, so that two uploads of the same content do not overwrite each other.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hodei_hub::blob::BlobWriteSession;

use crate::oci::failure::OciFailure;
use crate::oci::session_journal::{SessionJournal, SessionRecord, journal_now};

/// An upload session in progress.
///
/// The writer is the content store's write session: the
/// bytes go to its staging, and the commit is the only thing that publishes. With a
/// journal it may be **released** —this request's write
/// finished and the handle went back to the store—; reviving it is the job of
/// [`UploadRegistry::find_or_resume`].
pub struct UploadSession {
    /// The opaque session identifier.
    id: String,
    /// The content store's write in progress, if this process
    /// currently holds it.
    writer: Option<Box<dyn BlobWriteSession>>,
    /// The quota reservation that admitted this upload.
    ///
    /// It lives **here** and not in the request because the upload lives here and not
    /// in the request: the reservation of an abandoned or expired session
    /// returns to the ledger when the session dies, and what a session
    /// commits is settled on closing.
    ticket: crate::quota::QuotaTicket,
    /// When it was last touched.
    last_seen: Instant,
    /// The journal revision this registry read: the offset CAS
    /// advances **from here** and no blind write moves the
    /// authority. Zero when the registry has no durable journal.
    journal_version: u64,
    /// Whether the journal stored this session: a journaled session
    /// is resumed on another replica and its staging is durable.
    journaled: bool,
    /// The staging handle the authority stored: empty when the
    /// store does not resume.
    handle: String,
}

impl UploadSession {
    /// The session identifier.
    pub fn the_id(&self) -> &str {
        &self.id
    }

    /// The bytes that have come in, according to the local handle.
    ///
    /// `None` when the handle is released: this process does not know the
    /// offset without asking the authority.
    pub fn the_local_offset(&self) -> Option<u64> {
        self.writer.as_ref().map(|writer| writer.bytes_written())
    }

    /// The journal revision the registry holds in hand.
    pub fn the_journal_version(&self) -> u64 {
        self.journal_version
    }

    /// Records the revision the journal admitted after an advance.
    pub fn set_journal_version(&mut self, version: u64) {
        self.journal_version = version;
    }

    /// The staging handle the authority stored for this session.
    pub fn the_handle(&self) -> &str {
        &self.handle
    }

    /// Whether the journal stored this session: a journaled session
    /// is resumed on another replica and its staging is durable.
    pub fn is_journaled(&self) -> bool {
        self.journaled
    }

    /// The session's writer, for writing and for closing.
    ///
    /// `None` when the handle is released: the request that needs to
    /// write comes back through [`UploadRegistry::find_or_resume`], which
    /// revives it.
    pub fn the_writer(&mut self) -> Option<&mut Box<dyn BlobWriteSession>> {
        self.writer.as_mut()
    }

    /// The writer and the reservation, together and borrowed without fighting.
    ///
    /// Writing a chunk needs to take both at once: the writer
    /// for the byte and the quota to charge it **before** writing it.
    /// They are distinct fields, and the borrow says so in the type.
    pub fn the_writer_and_ticket(
        &mut self,
    ) -> Option<(
        &mut Box<dyn BlobWriteSession>,
        &mut crate::quota::QuotaTicket,
    )> {
        let writer = self.writer.as_mut()?;
        Some((writer, &mut self.ticket))
    }

    /// Settles the reservation against what the store gained on commit.
    pub fn settle(&mut self, stored_new_bytes: u64) {
        self.ticket.settle(stored_new_bytes);
    }

    /// Notes that the session has been touched.
    ///
    /// Expiry is counted from the last use, not from the
    /// start: a long upload lives as long as someone writes it, and
    /// a session nobody touches is a session to forget.
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Leaves in the store what is already durable and returns that offset.
    ///
    /// It is the offset the journal can admit: in the file store
    /// it is everything written —the synced file—; in
    /// multipart it is the chunks confirmed by the server, with what is
    /// in flight already collected. What remains in the buffer dies with the
    /// process and the client resends it against the `Range` the
    /// authority declares.
    pub async fn flush_durable(&mut self) -> Result<u64, hodei_hub::blob::BlobError> {
        match self.writer.as_mut() {
            Some(writer) => writer.flush_durable().await,
            // A released handle has nothing new to leave: the
            // authority already declared what it left.
            None => Ok(0),
        }
    }

    /// The handle the authority has released: there is nothing local to
    /// declare.
    pub fn the_durable_offset(&self) -> Option<u64> {
        self.writer
            .as_ref()
            .map(|writer| writer.the_durable_offset())
    }

    /// Whether the handle's digest covers the entire staging.
    ///
    /// A handle revived cold did not go through the bytes that were already
    /// in the store: whoever closes does not compare that partial digest against
    /// the declared one, and lets the store check the full
    /// content on publishing —the same honest verification, once—.
    pub fn the_digest_covers_all(&self) -> bool {
        match self.writer.as_ref() {
            Some(writer) => writer.the_digest_covers_all(),
            None => false,
        }
    }

    /// Releases the local handle when the journaled session no longer
    /// needs it.
    ///
    /// With a journal and handle, the store's staging is durable: closing
    /// the file —releasing its advisory lock— deletes **nothing**, and
    /// leaves the handle free for the next request to claim it here
    /// or on another replica. It is the hand that changes process, and it is what
    /// makes A's `PATCH` and B's `PATCH` exist at the same time.
    /// It is only released when nothing remains below what is durable: the
    /// multipart cannot release a buffer that still owes bytes to the
    /// store, and releasing it would abort its chunks —its handle stays
    /// until the commit, the abort or the sweep—.
    pub fn release_writer_when_possible(&mut self) {
        let Some(writer) = self.writer.as_mut() else {
            return;
        };
        if !self.journaled || self.handle.is_empty() {
            return;
        }
        if writer.the_durable_offset() != writer.bytes_written() {
            return;
        }
        if writer.detach().is_ok() {
            self.writer = None;
        }
    }

    /// Records the handle with which the authority reopened the session.
    fn set_handle(&mut self, handle: &str) {
        self.handle = handle.to_string();
    }
}

/// The result of advancing a journaled session's offset.
#[derive(Debug)]
pub enum Advance {
    /// The authority admitted the advance: the new revision.
    Admitted(u64),
    /// Another writer won the CAS: the offset the authority has. The
    /// protocol answers the `416` with the `Range` to continue from —with this,
    /// the replay of an already written chunk duplicates no part—.
    Conflict(u64),
    /// The session no longer exists for the authority (closed or swept).
    Gone,
    /// The authority did not respond.
    Unavailable(String),
}

/// The registry of upload sessions.
///
/// The map is under a **sync** mutex because its operations are short
/// and do not wait: what waits is the session, and each session has its
/// own async mutex. Two simultaneous uploads do not block each other.
///
/// With a durable journal, the map becomes a **cache**: the offset
/// authority is the journal, and a session this process does not know is
/// resumed from it ([`UploadRegistry::find_or_resume`]) with the revision
/// CAS. That is what makes the protocol survive a replica:
/// the bytes do not live here, they live in the CAS, and the staging handle
/// is enough to get back to them. The local TTL expires the **cache
/// entry**, not the session: an expired entry releases its handle without deleting
/// the durable staging, and the authority expires it on its own with the
/// same TTL ([`UploadRegistry::sweep`]).
pub struct UploadRegistry {
    /// The live sessions **in this process**, by identifier.
    sessions: Mutex<HashMap<String, Entry>>,
    /// How long a session lives without anyone touching it.
    ttl: Duration,
    /// How many sessions can live at once.
    max_sessions: usize,
    /// The durable offset authority: without it, sessions die
    /// with the process, as always.
    journal: Option<Arc<dyn SessionJournal>>,
}

impl Default for UploadRegistry {
    /// A registry with the usual limits.
    ///
    /// A session lives an hour and there are 64 at a time. They are limits
    /// a deployment can change, and the ones a deployment that does not
    /// change them has.
    fn default() -> Self {
        Self::new(Duration::from_secs(3600), 64)
    }
}

impl UploadRegistry {
    /// An empty registry, without a durable journal.
    ///
    /// The TTL is the time a session survives without anyone touching
    /// it, and the cap is how many sessions can exist at once: a
    /// registry without a cap is a registry where a client opens sessions and
    /// abandons them, and each one leaves a staging file on disk.
    pub fn new(ttl: Duration, max_sessions: usize) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            ttl,
            max_sessions,
            journal: None,
        }
    }

    /// A registry with a durable journal: the offset authority that
    /// survives the process and the replica.
    pub fn with_journal(
        ttl: Duration,
        max_sessions: usize,
        journal: Arc<dyn SessionJournal>,
    ) -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            ttl,
            max_sessions,
            journal: Some(journal),
        }
    }

    /// Opens a session and registers it.
    ///
    /// The identifier is a UUID: what the client receives in the
    /// `Location` and returns in each following request.
    ///
    /// The quota already decided before the call: the `ticket` is the reservation
    /// the upload brings with it, and the session keeps it until it is
    /// settled on commit or released on dying.
    pub async fn open(
        &self,
        account: &str,
        repository: &str,
        writer: Box<dyn BlobWriteSession>,
        ticket: crate::quota::QuotaTicket,
    ) -> Result<Arc<tokio::sync::Mutex<UploadSession>>, OciFailure> {
        let id = uuid::Uuid::new_v4().hyphenated().to_string();
        let handle = writer.the_staging_handle().unwrap_or_default();
        let journaled = self.journal.is_some();
        let record = SessionRecord {
            id: id.clone(),
            account: account.to_string(),
            repository: repository.to_string(),
            offset: writer.bytes_written(),
            handle: handle.clone(),
            version: 0,
            closed: false,
            touched_at_millis: journal_now(),
        };
        let session = Arc::new(tokio::sync::Mutex::new(UploadSession {
            id: id.clone(),
            writer: Some(writer),
            ticket,
            last_seen: Instant::now(),
            journal_version: 0,
            journaled,
            handle: handle.clone(),
        }));
        // The seat is reserved with the cap checked **under the same
        // lock acquisition**: two concurrent `POST`s cannot both slip
        // through the slot. The guard is released before any
        // await: a durable journal waits, and the map cannot sleep
        // with the lock in hand.
        {
            let mut sessions = self.lock();
            // Expired sessions go away before counting: a registry
            // that counts the dead cannot know whether one more fits.
            let expired = the_expired(&sessions, self.ttl);
            for old_id in &expired {
                sessions.remove(old_id);
            }
            if sessions.len() >= self.max_sessions {
                return Err(OciFailure::blob_upload_invalid(format!(
                    "there are already {} uploads in progress",
                    self.max_sessions
                )));
            }
            sessions.insert(
                id.clone(),
                Entry {
                    account: account.to_string(),
                    repository: repository.to_string(),
                    session: Arc::clone(&session),
                },
            );
        }
        // The authority writes after reserving the seat and before
        // the `Location` reaches the client: a session whose
        // journal failed does **not** exist —the seat is withdrawn and the staging
        // is cleaned up with the handle's Drop—, because a session that exists
        // only here is exactly the state this slice takes out of the
        // process.
        if let Some(journal) = &self.journal {
            if let Err(error) = journal.open(&record).await {
                self.remove(&id);
                return Err(journal_unavailable(error));
            }
            let mut session = session.lock().await;
            // The authority has the session: the staging becomes its own.
            // A local handle that dies —cache eviction, replica that
            // changes, process crash— does not delete bytes that another replica
            // can resume; what decides its fate is the journal and
            // its sweep.
            if !handle.is_empty()
                && let Some(writer) = session.the_writer()
            {
                writer.mark_durable();
            }
            // A `POST` that writes not a single byte can release the
            // handle now: the next request —here or on another replica— resumes it
            // through the handle.
            session.release_writer_when_possible();
        }
        Ok(session)
    }

    /// The session of an upload **on any replica**.
    ///
    /// First the local cache; if it does not exist here, or exists with the handle
    /// released, the authority: a session opened on another replica is
    /// resumed by reopening the CAS handle, with the offset the
    /// journal declares and the scope the registry demanded. A session of
    /// another account **does not exist** —it is not even resumed: the scope is
    /// checked against the authority before touching the store—.
    ///
    /// `Err` only when the authority did not respond: a session the
    /// journal has open cannot become a `404` because the
    /// journal was down —that is 503, and the client retries—.
    pub async fn find_or_resume(
        &self,
        account: &str,
        repository: &str,
        id: &str,
        store: &dyn hodei_hub::blob::BlobStore,
    ) -> Result<Option<Arc<tokio::sync::Mutex<UploadSession>>>, OciFailure> {
        if let Some(shared) = self.find(account, repository, id) {
            let reanimate = {
                let session = shared.lock().await;
                session.writer.is_none() && session.journaled && !session.handle.is_empty()
            };
            if !reanimate {
                return Ok(Some(shared));
            }
            // The handle was released —on opening the session or in another request—:
            // the authority states the offset and the store reopens the
            // staging.
            let record = self.the_journal_record(id).await?.ok_or_else(|| {
                OciFailure::blob_upload_unknown("the upload session does not exist")
            })?;
            if record.account != account || record.repository != repository {
                return Ok(None);
            }
            if !self.is_live(&record) {
                // Closed, or older than the TTL: the session is not there
                // any more, and the stale cache entry goes with it.
                self.remove(id);
                return Ok(None);
            }
            let writer = self.resume_writer(&record, store).await?;
            {
                let mut session = shared.lock().await;
                match writer {
                    Some(writer) => {
                        session.writer = Some(writer);
                        session.journal_version = record.version;
                        session.set_handle(&record.handle);
                    }
                    // A store that does not resume cannot reopen: the
                    // session exists and the handle was lost with the process
                    // that held it. The retry will land where there is a
                    // handle.
                    None => {
                        return Err(OciFailure::unavailable(
                            "the upload session cannot be resumed in this process",
                        ));
                    }
                }
            }
            return Ok(Some(shared));
        }
        let Some(journal) = &self.journal else {
            return Ok(None);
        };
        let record = journal.get(id).await.map_err(journal_unavailable)?;
        let Some(record) = record else {
            return Ok(None);
        };
        // The scope is part of the identity: a session of another account
        // or another repository does not exist, and the authority says so without
        // looking at the staging.
        if record.account != account || record.repository != repository || !self.is_live(&record) {
            return Ok(None);
        }
        let Some(writer) = self.resume_writer(&record, store).await? else {
            // The session exists for the authority and its handle is
            // empty: this deployment's store does not resume
            // writes. A `404` would deny a session the authority
            // has open; it is 503 with retry —on another replica with
            // a store that does resume, or when the sweep closes it,
            // the result will be the same—.
            return Err(OciFailure::unavailable(
                "the upload session cannot be resumed in this process",
            ));
        };
        // The local reservation that accompanies the handle: a ledger **without
        // limits** —the durable quota across replicas is not set by this
        // cache; it is set by the registry's quota reservation, and here it only
        // starts counting again what **this** replica writes—.
        let ticket = crate::quota::QuotaLedger::new(crate::quota::QuotaLimits::default())
            .begin(account, None)
            .map_err(|_| OciFailure::unavailable("the quota ledger is saturated"))?;
        let session = Arc::new(tokio::sync::Mutex::new(UploadSession {
            id: record.id.clone(),
            writer: Some(writer),
            ticket,
            last_seen: Instant::now(),
            journal_version: record.version,
            journaled: true,
            handle: record.handle.clone(),
        }));
        self.lock().insert(
            record.id.clone(),
            Entry {
                account: account.to_owned(),
                repository: repository.to_owned(),
                session: Arc::clone(&session),
            },
        );
        Ok(Some(session))
    }

    /// The path of the status `GET`: the offset the authority declares
    /// for a session this replica has not written.
    ///
    /// Returns the offset **without** reopening the store: querying how much
    /// an upload has does not need to take the handle nor exclude whoever
    /// has it.
    pub async fn the_journal_offset(
        &self,
        account: &str,
        repository: &str,
        id: &str,
    ) -> Result<Option<u64>, OciFailure> {
        let record = self.the_journal_record(id).await?;
        Ok(match record {
            Some(record)
                if record.account == account
                    && record.repository == repository
                    && self.is_live(&record) =>
            {
                Some(record.offset)
            }
            _ => None,
        })
    }

    /// Whether the authority still has the session alive: neither closed nor older
    /// than the TTL since it was last touched.
    ///
    /// The TTL is a promise of the protocol --after it, the session "is
    /// not there" and the client receives `BLOB_UPLOAD_UNKNOWN`--, and with a durable
    /// journal this read keeps the promise: the sweep only closes
    /// the record and aborts the staging, and does not run on each request.
    /// Without it, a staging that the startup cleanup already removed
    /// would keep being announced as a live upload (`204` with `Range`)
    /// that then cannot be resumed.
    fn is_live(&self, record: &SessionRecord) -> bool {
        let ttl_millis = self.ttl.as_millis() as u64;
        !record.closed && journal_now().saturating_sub(record.touched_at_millis) < ttl_millis
    }

    /// The session's authority, if the registry has an authority.
    async fn the_journal_record(&self, id: &str) -> Result<Option<SessionRecord>, OciFailure> {
        let Some(journal) = &self.journal else {
            return Ok(None);
        };
        journal.get(id).await.map_err(journal_unavailable)
    }

    /// Reopens the CAS handle over the handle the authority stored.
    ///
    /// A store that does not resume (the in-memory one) or an empty handle
    /// return `None`: the session **exists** for the authority and
    /// cannot be written from the cache.
    async fn resume_writer(
        &self,
        record: &SessionRecord,
        store: &dyn hodei_hub::blob::BlobStore,
    ) -> Result<Option<Box<dyn BlobWriteSession>>, OciFailure> {
        if record.handle.is_empty() {
            return Ok(None);
        }
        let writer = store
            .resume(&record.handle, record.offset)
            .await
            .map_err(the_resume_error)?;
        Ok(Some(writer))
    }

    /// The advance of a session's offset, arbitrated by the journal.
    ///
    /// The registry calls **after** writing the chunk: the authority
    /// only admits the offset of the bytes that are already in the CAS —that is
    /// why it advances up to `flush_durable`, not up to what the handle's
    /// buffer holds in the process—. A CAS conflict is not a failure
    /// —it is the protocol's `416`: the other writer won, the authority
    /// says where to continue—.
    pub async fn advance(&self, id: &str, expected_version: u64, offset: u64) -> Advance {
        let Some(journal) = &self.journal else {
            return Advance::Admitted(expected_version);
        };
        match journal
            .advance(id, expected_version, offset, journal_now())
            .await
        {
            Ok(version) => Advance::Admitted(version),
            Err(crate::oci::session_journal::JournalError::Conflict { offset, .. }) => {
                Advance::Conflict(offset)
            }
            Err(crate::oci::session_journal::JournalError::NoSuchSession { .. }) => Advance::Gone,
            Err(error) => Advance::Unavailable(error.to_string()),
        }
    }

    /// Closes a session's authority: committed or aborted.
    pub async fn close(&self, id: &str) -> Result<(), OciFailure> {
        if let Some(journal) = &self.journal {
            journal.close(id).await.map_err(journal_unavailable)?;
        }
        self.remove(id);
        Ok(())
    }

    /// The durable sweep: sessions **another replica** left
    /// open when it fell are marked closed and their orphaned staging is
    /// aborted against the store. It is the AC3 reconciliation: without this,
    /// a crash leaves a multipart open forever with bytes that
    /// nobody verified. Idempotent: closing twice is closing once.
    pub async fn sweep(&self, store: &dyn hodei_hub::blob::BlobStore) -> Result<usize, OciFailure> {
        let Some(journal) = &self.journal else {
            return Ok(0);
        };
        let ttl_millis = self.ttl.as_millis() as u64;
        let expired = journal
            .sweep_expired(journal_now(), ttl_millis)
            .await
            .map_err(|error| {
                OciFailure::unavailable(format!("the session sweep failed: {error}"))
            })?;
        let mut reconciled = 0usize;
        for record in &expired {
            // A session of this process with the seat taken is not
            // an orphan: its Drop or its abort take care of it. Only what this
            // process does not have is reconciled here.
            if self.lock().contains_key(&record.id) {
                continue;
            }
            if record.handle.is_empty() {
                // This staging's store does not resume: its staging
                // died with the process; the authority already closed it.
                reconciled += 1;
                continue;
            }
            // Reopen to abort: a staging alive on another replica cannot
            // be taken (the advisory lock prevents it), and neither can one that
            // is no longer there. What is taken is closed **without
            // publishing**: abort, never commit.
            if let Ok(mut writer) = store.resume(&record.handle, record.offset).await {
                let _ = writer.abort().await;
                reconciled += 1;
            }
        }
        Ok(reconciled)
    }

    /// The session of an upload, if it belongs to this account and this repository.
    ///
    /// A session of another account **does not exist**: the same error as a
    /// session that is not there. A `404` that distinguished the two would say that in
    /// the other account there is an upload in progress.
    pub fn find(
        &self,
        account: &str,
        repository: &str,
        id: &str,
    ) -> Option<Arc<tokio::sync::Mutex<UploadSession>>> {
        let mut sessions = self.lock();
        let expired = the_expired(&sessions, self.ttl);
        for expired_id in &expired {
            sessions.remove(expired_id);
        }
        let entry = sessions.get(id)?;
        // The account and the repository are compared **without** taking the
        // session's mutex: a session that is being written is not a
        // session that does not exist, and a client that sends two chunks at the
        // same time cannot receive a "does not exist" because the
        // other chunk has it taken.
        if entry.account != account || entry.repository != repository {
            return None;
        }
        Some(Arc::clone(&entry.session))
    }

    /// Closes a session: removes it from the registry.
    ///
    /// The session that is removed is released, and on release its CAS
    /// write is closed. With a journal the staging is **durable**: releasing the
    /// handle deletes nothing —an expired seat is not an abandonment: the
    /// upload may continue on another replica—; only what is truly aborted or
    /// committed touches the store.
    pub fn remove(&self, id: &str) {
        self.lock().remove(id);
    }

    /// How many sessions are alive.
    pub fn the_number_of_sessions(&self) -> usize {
        self.lock().len()
    }

    /// The map's mutex.
    ///
    /// A poison here means a thread died holding the mutex, and
    /// a session registry that cannot be read is a registry that cannot
    /// say no: it is reset.
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A journal error translated to the protocol: the authority did not
/// respond, and a journaled session cannot become invisible.
fn journal_unavailable(error: impl std::fmt::Display) -> OciFailure {
    OciFailure::unavailable(format!("the session journal did not answer: {error}"))
}

/// A store rejection on reopening, translated to the protocol.
///
/// The session **exists** —the authority has it open—: what cannot
/// be is writing here now. A `404` would lie about the
/// session's existence.
fn the_resume_error(error: hodei_hub::blob::BlobError) -> OciFailure {
    match error {
        hodei_hub::blob::BlobError::Unavailable(message) => {
            OciFailure::unavailable(format!("the upload could not be resumed: {message}"))
        }
        error => OciFailure::unavailable(format!("the content store refused the resume: {error}")),
    }
}

/// A registry entry: the session and whose it is.
///
/// The account and the repository live **here**, and not inside the session:
/// checking whose a session is must not have to wait for the session
/// to be free.
struct Entry {
    /// The account that opened it.
    account: String,
    /// The repository it uploads to.
    repository: String,
    /// The session.
    session: Arc<tokio::sync::Mutex<UploadSession>>,
}

/// The identifiers of the sessions that already expired.
fn the_expired(sessions: &HashMap<String, Entry>, ttl: Duration) -> Vec<String> {
    let now = Instant::now();
    sessions
        .iter()
        .filter_map(|(id, entry)| {
            // A session that is busy does not expire: someone is
            // writing it, and a write in progress is not an
            // abandoned session.
            let expired = entry
                .session
                .try_lock()
                .map(|session| now.duration_since(session.last_seen) >= ttl)
                .unwrap_or(false);
            expired.then(|| id.clone())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oci::session_journal::{MemoryJournal, SessionJournal};
    use hodei_hub::blob::{BlobStore, InMemoryBlobStore};

    /// A reservation from an unlimited ledger: the session tests
    /// test sessions, not quotas.
    fn an_unlimited_ticket() -> crate::quota::QuotaTicket {
        crate::quota::QuotaLedger::new(crate::quota::QuotaLimits::default())
            .begin("acme", None)
            .expect("an unlimited ledger admits everything")
    }

    /// A session that belongs to another account does not exist for this one.
    #[tokio::test]
    async fn a_session_of_another_account_does_not_exist() {
        let registry = UploadRegistry::new(Duration::from_secs(60), 8);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        assert!(registry.find("acme", "releases", &id).is_some());
        assert!(
            registry.find("other", "releases", &id).is_none(),
            "the session of another account was found"
        );
        assert!(
            registry.find("acme", "private", &id).is_none(),
            "the session of another repository was found"
        );
    }

    /// A session nobody touches expires, and the slot is freed.
    #[tokio::test]
    async fn a_session_that_nobody_touches_expires() {
        let registry = UploadRegistry::new(Duration::from_millis(1), 8);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        tokio::time::sleep(Duration::from_millis(5)).await;
        assert!(
            registry.find("acme", "releases", &id).is_none(),
            "the expired session was still there"
        );
    }

    /// The in-memory store does not resume: a journaled session of its own
    /// keeps the local handle —there is no handle to release—.
    #[tokio::test]
    async fn a_session_without_a_handle_keeps_its_writer() {
        let journal = Arc::new(MemoryJournal::new());
        let registry = UploadRegistry::with_journal(Duration::from_secs(60), 8, journal);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let session = session.lock().await;
        assert!(
            session.the_local_offset().is_some(),
            "a store that cannot resume lost its writer"
        );
        assert!(session.the_handle().is_empty());
    }

    /// A journaled session that expired does not exist: neither the status nor the
    /// resumption announces it, even if the sweep has not closed it.
    #[tokio::test]
    async fn a_journaled_session_past_its_ttl_is_not_there() {
        let journal = Arc::new(MemoryJournal::new());
        let registry = UploadRegistry::with_journal(Duration::from_millis(30), 8, journal);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();
        assert!(
            registry
                .the_journal_offset("acme", "releases", &id)
                .await
                .expect("the journal answers")
                .is_some(),
            "a fresh session is not announced"
        );

        std::thread::sleep(Duration::from_millis(60));
        assert!(
            registry
                .the_journal_offset("acme", "releases", &id)
                .await
                .expect("the journal answers")
                .is_none(),
            "an expired session is still announced"
        );
    }

    /// A journaled session of another account is NOT revived: the
    /// authority has the session, and the scope is checked against it.
    #[tokio::test]
    async fn an_alien_session_is_not_resumable() {
        let journal = Arc::new(MemoryJournal::new());
        let registry = UploadRegistry::with_journal(Duration::from_secs(60), 8, journal);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        let other = registry
            .find_or_resume("other", "releases", &id, &store)
            .await
            .expect("the journal answers");
        assert!(
            other.is_none(),
            "the session of another account was resumed"
        );
        let other_repo = registry
            .find_or_resume("acme", "private", &id, &store)
            .await
            .expect("the journal answers");
        assert!(
            other_repo.is_none(),
            "the session of another repository was resumed"
        );
        assert!(
            registry
                .the_journal_offset("other", "releases", &id)
                .await
                .expect("the journal answers")
                .is_none(),
            "the offset of an alien session was readable"
        );
    }

    /// The journal CAS does not let a writer with an old revision
    /// advance: it is the `416` that avoids duplicating a resent chunk.
    #[tokio::test]
    async fn the_cas_refuses_a_stale_writer() {
        let journal: Arc<dyn SessionJournal> = Arc::new(MemoryJournal::new());
        let registry =
            UploadRegistry::with_journal(Duration::from_secs(60), 8, Arc::clone(&journal));
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        // The first advance goes in: v0 -> v1, offset 10.
        match registry.advance(&id, 0, 10).await {
            Advance::Admitted(1) => {}
            other => panic!("the first advance was not admitted: {other:?}"),
        }
        // A writer with the old revision does not move the authority.
        match registry.advance(&id, 0, 999).await {
            Advance::Conflict(10) => {}
            other => panic!("the stale writer was not refused: {other:?}"),
        }
        // The authority stays at what the winner wrote.
        let record = journal.get(&id).await.expect("journal").expect("record");
        assert_eq!(record.offset, 10);
        assert_eq!(record.version, 1);
    }

    /// A session that is being written keeps existing.
    ///
    /// Checking whose a session is cannot depend on the
    /// session being free: a client that sends two chunks at the same time --or
    /// that retries while the other is in flight-- cannot receive a "does not
    /// exist" because the session is taken by another.
    #[tokio::test]
    async fn a_session_that_is_being_written_is_still_there() {
        let registry = UploadRegistry::new(Duration::from_secs(60), 8);
        let store = InMemoryBlobStore::empty();
        let session = registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
            .expect("the session opens");
        let id = session.lock().await.the_id().to_string();

        // The session is taken, as if a chunk were being
        // written right now.
        let taken = session.lock().await;
        assert!(
            registry.find("acme", "releases", &id).is_some(),
            "a session that is being written was not found"
        );
        drop(taken);
    }

    /// The session cap is respected: a client that opens and abandons
    /// sessions cannot fill the registry.
    #[tokio::test]
    async fn the_number_of_sessions_is_capped() {
        let registry = UploadRegistry::new(Duration::from_secs(60), 2);
        let store = InMemoryBlobStore::empty();
        for _ in 0..2 {
            registry
                .open(
                    "acme",
                    "releases",
                    store.begin().await.expect("the session"),
                    an_unlimited_ticket(),
                )
                .await
                .expect("the session opens");
        }
        let error = match registry
            .open(
                "acme",
                "releases",
                store.begin().await.expect("the session"),
                an_unlimited_ticket(),
            )
            .await
        {
            Ok(_) => panic!("the third session does not fit"),
            Err(error) => error,
        };
        assert_eq!(error.the_code(), "BLOB_UPLOAD_INVALID");
    }
}
