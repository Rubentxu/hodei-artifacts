//! The durable journal of upload sessions (HSP-201).
//!
//! The map of [`super::uploads::UploadRegistry`] is the local cache of what
//! is the authority here: an upload's session lives **between
//! requests**, and in a deployment with two replicas a request lands on
//! either of the two. What has to survive the process is
//! minimal and closed: the identifier, the account and the repository it
//! belongs to, the byte offset written so far, the store's staging
//! handle, the revision version for the CAS and the date of
//! its last contact for the TTL. What is **not** stored here are the
//! bytes: those live in the CAS, and the handle is the only way back
//! to them.
//!
//! The offset CAS is what makes resumption safe: each advance
//! requires the revision the writer read and leaves a **revision
//! marker** under the `@upsert` key `{key}#v{revision}`. Two
//! concurrent writers fight for the same marker; the server
//! arbitrates, the loser writes nothing, rereads the real offset and the
//! protocol answers the `416` that says where to continue. With that, the
//! replay of a chunk that already went in duplicates no part.
//!
//! The implementations:
//!
//! - [`MemoryJournal`] —the authority shared between two registries of the
//!   same process: the seam with which the two-replica protocol is
//!   tested without a server.
//! - [`DgraphJournal`] —the authority on the graph, with its own **additive**
//!   schema (`hodei.upload.session.*`) and the same CAS the quota
//!   ledger (HSP-198) uses: the revision marker is the
//!   condition, not a client notice.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{
    GraphClient, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
};
use serde_json::Value;

/// Why the journal could not do what it was asked.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    /// The session does not exist, is closed, or the advance collided with a
    /// close: in all three cases the protocol's response is the
    /// same —a session that is not there—.
    #[error("the upload session '{id}' does not exist")]
    NoSuchSession {
        /// The requested identifier.
        id: String,
    },
    /// Another advance won the CAS. The registry rereads the real offset and the
    /// protocol answers the `416` with the `Range` that says where to
    /// continue.
    #[error("the upload session '{id}' moved: it is at offset {offset} revision {revision}")]
    Conflict {
        /// The session identifier.
        id: String,
        /// The current offset according to the authority.
        offset: u64,
        /// The current revision according to the authority.
        revision: u64,
    },
    /// The journal did not respond.
    #[error("the session journal is not available: {0}")]
    Unavailable(String),
}

/// What the journal remembers about a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    /// The opaque session identifier.
    pub id: String,
    /// The account that opened it: a session of another account does not exist.
    pub account: String,
    /// The repository it uploads to.
    pub repository: String,
    /// The bytes written so far —the upload's offset—.
    pub offset: u64,
    /// The store's staging handle; empty if the store does not
    /// resume writes.
    pub handle: String,
    /// The journal revision for the CAS: each advance raises it by
    /// one and the writer assumes the one it read.
    pub version: u64,
    /// Closed: committed, aborted or expired. A closed session is not
    /// resumed.
    pub closed: bool,
    /// The last contact in millis since the epoch: the durable TTL is
    /// counted from here.
    pub touched_at_millis: u64,
}

/// The session journal: the authority that survives the process.
#[async_trait]
pub trait SessionJournal: Send + Sync {
    /// Records an open session.
    async fn open(&self, record: &SessionRecord) -> Result<(), JournalError>;

    /// The session by its identifier, open or closed.
    async fn get(&self, id: &str) -> Result<Option<SessionRecord>, JournalError>;

    /// Advances the offset **with CAS**: `expected_version` is the revision
    /// the writer read; if the authority is at another, the advance
    /// writes nothing and returns [`JournalError::Conflict`] with the real
    /// offset. Success raises the revision by one and refreshes the TTL
    /// with `touched_at_millis`.
    async fn advance(
        &self,
        id: &str,
        expected_version: u64,
        offset: u64,
        touched_at_millis: u64,
    ) -> Result<u64, JournalError>;

    /// Closes a session. Idempotent: closing something already closed is a
    /// true no-op —publication and the sweep compete, and whoever
    /// arrives first decides—.
    async fn close(&self, id: &str) -> Result<(), JournalError>;

    /// The TTL sweep: open sessions whose last contact
    /// exceeds the grace are marked closed and returned **with their
    /// record** so that the journal's owner aborts the staging they
    /// left behind. Without this, a crash leaves open sessions and orphaned
    /// multipart uploads forever.
    async fn sweep_expired(
        &self,
        now_millis: u64,
        ttl_millis: u64,
    ) -> Result<Vec<SessionRecord>, JournalError>;
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The in-memory authority, shared between two registries of the same
/// process: the seam with which the two-replica protocol is tested
/// without a server. The CAS is a lock on the table; Dgraph's is
/// the server.
#[derive(Default)]
pub struct MemoryJournal {
    records: std::sync::Mutex<HashMap<String, SessionRecord>>,
}

impl MemoryJournal {
    /// An empty journal.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, SessionRecord>> {
        self.records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[async_trait]
impl SessionJournal for MemoryJournal {
    async fn open(&self, record: &SessionRecord) -> Result<(), JournalError> {
        let mut records = self.lock();
        if records.contains_key(&record.id) {
            return Err(JournalError::Unavailable(format!(
                "the session '{}' already stands in the journal",
                record.id
            )));
        }
        records.insert(record.id.clone(), record.clone());
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<SessionRecord>, JournalError> {
        Ok(self.lock().get(id).cloned())
    }

    async fn advance(
        &self,
        id: &str,
        expected_version: u64,
        offset: u64,
        touched_at_millis: u64,
    ) -> Result<u64, JournalError> {
        let mut records = self.lock();
        let Some(record) = records.get_mut(id) else {
            return Err(JournalError::NoSuchSession { id: id.to_owned() });
        };
        if record.closed {
            return Err(JournalError::NoSuchSession { id: id.to_owned() });
        }
        if record.version != expected_version {
            return Err(JournalError::Conflict {
                id: id.to_owned(),
                offset: record.offset,
                revision: record.version,
            });
        }
        record.version += 1;
        record.offset = offset;
        record.touched_at_millis = touched_at_millis;
        Ok(record.version)
    }

    async fn close(&self, id: &str) -> Result<(), JournalError> {
        if let Some(record) = self.lock().get_mut(id) {
            record.closed = true;
        }
        Ok(())
    }

    async fn sweep_expired(
        &self,
        now: u64,
        ttl_millis: u64,
    ) -> Result<Vec<SessionRecord>, JournalError> {
        let mut records = self.lock();
        let expired: Vec<SessionRecord> = records
            .values()
            .filter(|record| {
                !record.closed && now.saturating_sub(record.touched_at_millis) >= ttl_millis
            })
            .cloned()
            .collect();
        for record in &expired {
            if let Some(entry) = records.get_mut(&record.id) {
                entry.closed = true;
            }
        }
        Ok(expired)
    }
}

/// The journal's additive type.
pub const SESSION_TYPE: &str = "HodeiUploadSession";

/// The journal's **additive** DDL: its own predicates, an `@upsert`
/// index on the key —which is what serializes a session's mutations and
/// makes the revalidation of the filter within each transaction be the
/// CAS—, and a new type. It redeclares nothing that another slice
/// owns.
pub const SESSION_SCHEMA_DQL: &str = "
hodei.upload.session.key: string @index(exact) @upsert .
hodei.upload.session.account: string @index(exact) .
hodei.upload.session.tenant: string @index(exact) .
hodei.upload.session.repository: string .
hodei.upload.session.offset: int .
hodei.upload.session.handle: string .
hodei.upload.session.version: int .
hodei.upload.session.closed: bool .
hodei.upload.session.touched_at: int .
type HodeiUploadSession {
  hodei.upload.session.key
  hodei.upload.session.account
  hodei.upload.session.tenant
  hodei.upload.session.repository
  hodei.upload.session.offset
  hodei.upload.session.handle
  hodei.upload.session.version
  hodei.upload.session.closed
  hodei.upload.session.touched_at
}
";

/// The authority on the graph: the same CAS as the quota ledger,
/// arbitrated by the server. Cloneable and light —the client is an `Arc`
/// and the tenant a name—, because two writers of the same session
/// are two holders of the same journal.
#[derive(Clone)]
pub struct DgraphJournal {
    client: Arc<GraphClient>,
    tenant: String,
}

impl DgraphJournal {
    /// Opens the journal on the graph, applying its additive DDL.
    pub async fn open(client: Arc<GraphClient>, tenant: &str) -> Result<Self, JournalError> {
        if tenant.is_empty()
            || !tenant
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(JournalError::Unavailable(format!(
                "the journal tenant '{tenant}' is not a safe key part"
            )));
        }
        apply_session_schema(&client).await?;
        Ok(Self {
            client,
            tenant: tenant.to_owned(),
        })
    }

    /// The key of a session. The tenant is validated and the id is a
    /// UUID: neither part introduces the separator, and the
    /// key is stable for two writers of the same id.
    fn key_of(&self, id: &str) -> String {
        format!("upload-session/{}/{}", self.tenant, id)
    }

    /// The key of a revision's marker: the CAS conflict anchor,
    /// created only if missing —which is what forces two writers
    /// of the same revision to fight for the same `@upsert` key.
    fn marker_key(&self, id: &str, revision: u64) -> String {
        format!("{}#v{revision}", self.key_of(id))
    }

    async fn read(&self, id: &str) -> Result<Option<SessionRecord>, JournalError> {
        let query = format!(
            "{{ q(func: eq(hodei.upload.session.key, {key})) {{ \
             hodei.upload.session.account hodei.upload.session.repository \
             hodei.upload.session.offset hodei.upload.session.handle \
             hodei.upload.session.version hodei.upload.session.closed \
             hodei.upload.session.touched_at }} }}",
            key = dql_string(&self.key_of(id)),
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| JournalError::Unavailable(error.to_string()))?;
        require_no_errors(&response.json)?;
        let nodes = response
            .json
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let Some(node) = nodes.first() else {
            return Ok(None);
        };
        Ok(Some(SessionRecord {
            id: id.to_owned(),
            account: text_of(node, "hodei.upload.session.account").unwrap_or_default(),
            repository: text_of(node, "hodei.upload.session.repository").unwrap_or_default(),
            offset: node
                .get("hodei.upload.session.offset")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            handle: text_of(node, "hodei.upload.session.handle").unwrap_or_default(),
            version: node
                .get("hodei.upload.session.version")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            closed: node
                .get("hodei.upload.session.closed")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            touched_at_millis: node
                .get("hodei.upload.session.touched_at")
                .and_then(Value::as_u64)
                .unwrap_or(0),
        }))
    }
}

#[async_trait]
impl SessionJournal for DgraphJournal {
    async fn open(&self, record: &SessionRecord) -> Result<(), JournalError> {
        let key = self.key_of(&record.id);
        let nquads = format!(
            "_:s <dgraph.type> \"{session_type}\" .\n\
             _:s <hodei.upload.session.key> {key} .\n\
             _:s <hodei.upload.session.tenant> {tenant} .\n\
             _:s <hodei.upload.session.account> {account} .\n\
             _:s <hodei.upload.session.repository> {repository} .\n\
             _:s <hodei.upload.session.offset> \"{offset}\" .\n\
             _:s <hodei.upload.session.handle> {handle} .\n\
             _:s <hodei.upload.session.version> \"{version}\" .\n\
             _:s <hodei.upload.session.closed> \"false\" .\n\
             _:s <hodei.upload.session.touched_at> \"{touched}\" .\n",
            session_type = SESSION_TYPE,
            key = dql_string(&key),
            tenant = dql_string(&self.tenant),
            account = dql_string(&record.account),
            repository = dql_string(&record.repository),
            offset = record.offset,
            handle = dql_string(&record.handle),
            version = record.version,
            touched = record.touched_at_millis,
        );
        let response = self
            .client
            .upsert(GraphUpsertTransaction {
                query: format!(
                    "{{ q as var(func: eq(hodei.upload.session.key, {key})) }}",
                    key = dql_string(&key),
                ),
                blocks: vec![GraphMutationBlock::set_if("eq(len(q), 0)", nquads)],
                commit_now: true,
            })
            .await
            .map_err(|error| JournalError::Unavailable(error.to_string()))?;
        require_no_errors(&response.json)
    }

    async fn get(&self, id: &str) -> Result<Option<SessionRecord>, JournalError> {
        self.read(id).await
    }

    async fn advance(
        &self,
        id: &str,
        expected_version: u64,
        offset: u64,
        touched_at_millis: u64,
    ) -> Result<u64, JournalError> {
        let key = self.key_of(id);
        let marker = self.marker_key(id, expected_version + 1);
        // The transaction: the session revalidated IN ITS OWN SNAPSHOT at
        // the revision the writer assumed and not closed, and the
        // marker of the next revision STILL NONEXISTENT. The filter
        // is re-evaluated under the serialization of the `@upsert` index of the
        // key, so that two advances of the same revision cannot
        // both match: the second sees the moved revision or the marker
        // set, its block writes nothing and it loses.
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{\n  s as var(func: eq(hodei.upload.session.key, {key})) \
                 @filter(eq(hodei.upload.session.version, {expected}) AND eq(hodei.upload.session.closed, false))\n  \
                 t as var(func: eq(hodei.upload.session.key, {marker}))\n}}",
                key = dql_string(&key),
                expected = expected_version,
                marker = dql_string(&marker),
            ),
            blocks: vec![GraphMutationBlock::set_if(
                "eq(len(s), 1) AND eq(len(t), 0)",
                format!(
                    "uid(s) <hodei.upload.session.offset> \"{offset}\" .\n\
                     uid(s) <hodei.upload.session.version> \"{next}\" .\n\
                     uid(s) <hodei.upload.session.touched_at> \"{touched}\" .\n\
                     _:t <dgraph.type> \"{session_type}\" .\n\
                     _:t <hodei.upload.session.key> {marker} .\n",
                    offset = offset,
                    next = expected_version + 1,
                    touched = touched_at_millis,
                    session_type = SESSION_TYPE,
                    marker = dql_string(&marker),
                ),
            )],
            commit_now: true,
        };
        // Two concurrent transactions on the same `@upsert` may
        // end in a server **serialization abort**, not in
        // "I lost the CAS": the abort is retried —the filter's
        // revalidation will then say who won—; a failure that is not an abort is the
        // journal not responding. The verdict is not read from the upsert's
        // response —`as var`s are not projected—: the state is reread
        // AFTER the commit, the same proven rule as the quota
        // ledger (HSP-171): the transaction won if the revision it
        // wrote is the one that is there.
        for attempt in 1..=3 {
            match self.client.upsert(transaction.clone()).await {
                Err(error) => {
                    let text = error.to_string();
                    let abort = text.contains("aborted")
                        || text.contains("Aborted")
                        || text.contains("please retry")
                        || text.contains("RETRY");
                    if !abort {
                        return Err(JournalError::Unavailable(text));
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(25 * attempt)).await;
                }
                Ok(response) => {
                    require_no_errors(&response.json)?;
                    return match self.read(id).await? {
                        None => Err(JournalError::NoSuchSession { id: id.to_owned() }),
                        Some(record) if record.closed => {
                            Err(JournalError::NoSuchSession { id: id.to_owned() })
                        }
                        Some(record)
                            if record.version == expected_version + 1
                                && record.offset == offset =>
                        {
                            Ok(expected_version + 1)
                        }
                        // My transaction did not write or someone moved the
                        // authority: the protocol's `416` carries the real
                        // offset and revision.
                        Some(record) => Err(JournalError::Conflict {
                            id: id.to_owned(),
                            offset: record.offset,
                            revision: record.version,
                        }),
                    };
                }
            }
        }
        // The server did not stop aborting: another writer holds the session
        // at a revision this one did not reach. It is reread one last time
        // to say how much.
        match self.read(id).await? {
            None => Err(JournalError::NoSuchSession { id: id.to_owned() }),
            Some(record) => Err(JournalError::Conflict {
                id: id.to_owned(),
                offset: record.offset,
                revision: record.version,
            }),
        }
    }

    async fn close(&self, id: &str) -> Result<(), JournalError> {
        let key = self.key_of(id);
        let response = self
            .client
            .upsert(GraphUpsertTransaction {
                query: format!(
                    "{{ s as var(func: eq(hodei.upload.session.key, {key})) \
                     @filter(eq(hodei.upload.session.closed, false)) }}",
                    key = dql_string(&key),
                ),
                blocks: vec![GraphMutationBlock::set_if(
                    "eq(len(s), 1)",
                    "uid(s) <hodei.upload.session.closed> \"true\" .\n",
                )],
                commit_now: true,
            })
            .await
            .map_err(|error| JournalError::Unavailable(error.to_string()))?;
        require_no_errors(&response.json)
    }

    async fn sweep_expired(
        &self,
        now: u64,
        ttl_millis: u64,
    ) -> Result<Vec<SessionRecord>, JournalError> {
        let deadline = now.saturating_sub(ttl_millis);
        // The sweep is **scoped to the tenant**: on a shared
        // alpha, a global census of open sessions would close the
        // sessions of others —that is forbidden by the environment's
        // contract—. The `tenant` key is the boundary.
        let response = self
            .client
            .execute_dql(&format!(
                "{{ q(func: eq(hodei.upload.session.tenant, {tenant})) \
                 @filter(eq(hodei.upload.session.closed, false) AND \
                 le(hodei.upload.session.touched_at, {deadline})) {{ \
                 hodei.upload.session.key hodei.upload.session.account \
                 hodei.upload.session.repository hodei.upload.session.offset \
                 hodei.upload.session.handle hodei.upload.session.version \
                 hodei.upload.session.touched_at }} }}",
                tenant = dql_string(&self.tenant),
                deadline = deadline,
            ))
            .await
            .map_err(|error| JournalError::Unavailable(error.to_string()))?;
        require_no_errors(&response.json)?;
        let nodes = response
            .json
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut expired = Vec::new();
        for node in nodes {
            let key = match text_of(&node, "hodei.upload.session.key") {
                Some(key) => key,
                None => continue,
            };
            let version = node
                .get("hodei.upload.session.version")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            // The close mark relies on the version read: if someone
            // resumed and advanced the session between the listing and the mark, the
            // filter does not match, the sweep does not touch it and the next
            // pass decides. Closing does not raise the version —there is no CAS to lose:
            // two closes of the same state are the same fact—.
            let closed = self
                .client
                .upsert(GraphUpsertTransaction {
                    query: format!(
                        "{{ s as var(func: eq(hodei.upload.session.key, {key})) \
                         @filter(eq(hodei.upload.session.version, {version}) AND eq(hodei.upload.session.closed, false)) }}",
                        key = dql_string(&key),
                        version = version,
                    ),
                    blocks: vec![GraphMutationBlock::set_if(
                        "eq(len(s), 1)",
                        "uid(s) <hodei.upload.session.closed> \"true\" .\n",
                    )],
                    commit_now: true,
                })
                .await
                .map_err(|error| JournalError::Unavailable(error.to_string()))?;
            require_no_errors(&closed.json)?;
            // The verdict is reread, not read from the response: the
            // server does not project the `as var` variables of the upsert.
            // A session that is still open is a session that another
            // writer moved —the filter does not match— and the next
            // pass decides it.
            let id = key.rsplit('/').next().unwrap_or(&key).to_owned();
            let marked = match self.read(&id).await? {
                Some(record) if record.closed => record,
                _ => continue,
            };
            expired.push(SessionRecord {
                id,
                account: marked.account,
                repository: marked.repository,
                offset: marked.offset,
                handle: marked.handle,
                version: marked.version,
                closed: true,
                touched_at_millis: marked.touched_at_millis,
            });
        }
        Ok(expired)
    }
}

/// The journal's DDL is additive and idempotent; two simultaneous
/// openings alter the same slice and the server may reject one
/// while it settles the indexes: it is retried a little and fails late.
async fn apply_session_schema(client: &GraphClient) -> Result<(), JournalError> {
    let mut last = String::new();
    for attempt in 1..=3 {
        match client.alter_schema(SESSION_SCHEMA_DQL).await {
            Ok(()) => return Ok(()),
            Err(error) if attempt < 3 => {
                last = error.to_string();
                tokio::time::sleep(std::time::Duration::from_millis(150 * attempt as u64)).await;
            }
            Err(error) => return Err(JournalError::Unavailable(error.to_string())),
        }
    }
    Err(JournalError::Unavailable(format!(
        "the session DDL slice did not settle: {last}"
    )))
}

/// Errors that the wire carries inside the JSON are never read as
/// success.
fn require_no_errors(json: &Value) -> Result<(), JournalError> {
    let empty: Vec<Value> = Vec::new();
    let errors = json
        .get("errors")
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    if errors.is_empty() {
        return Ok(());
    }
    let text = errors
        .iter()
        .filter_map(|error| error.get("message").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("; ");
    Err(JournalError::Unavailable(if text.is_empty() {
        format!("{errors:?}")
    } else {
        text
    }))
}

fn text_of(node: &Value, predicate: &str) -> Option<String> {
    node.get(predicate)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The journal clock: millis since the epoch, the same unit as the
/// graph's dates.
pub fn journal_now() -> u64 {
    now_millis()
}
