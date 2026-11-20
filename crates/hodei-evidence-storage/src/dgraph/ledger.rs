//! The ledger of events, over the graph authority.
//!
//! This is the Dgraph port of [`crate::surreal::ledger`]: the same
//! observable semantics — proven by the ledger contract — implemented on
//! the store the graph-first deployment runs. What is ported is the
//! SEMANTICS, not the file: the record-identity trick of the local
//! engine (name the row by its position) becomes here the combination
//! of the two primitives the stack already proves:
//!
//! - **The head of a stream is the guard of HSP-184.** A stream is a
//!   scope (`ledger|tenant|stream-id`); the guard revision counts the
//!   committed appends, so the next free position IS the revision. An
//!   append demands the exact revision its position names — a CAS on
//!   the sequence itself. The server forces the conflict through the
//!   guard's `@upsert` token: two writers that claim the same position
//!   serialize, one commits, the loser revalidates and is refused. No
//!   read-then-write race.
//! - **The position is once-only twice over.** Every event node carries
//!   `hodei.ledger.key` = `sha256(tenant#stream)#position`, exact and
//!   `@upsert`. Even a writer that bypassed the guard could not give
//!   one position two nodes: the server refuses it.
//! - **A delivery is an operation.** The operation id is derived from
//!   (tenant, stream, position, event id) and commits in the same
//!   transaction as the event and the guard advance. A lost ACK replayed
//!   with the same envelope finds its marker and returns the committed
//!   event id without writing anything — the same delivery arriving
//!   twice writes once, even after the stream moved, and the registry
//!   is consulted BEFORE the CAS so a repetition is never reported as a
//!   conflict.
//!
//! The three outcomes of an append are told apart, as the contract
//! demands: a repeated delivery is `Ok` (its committed identity), a
//! different event at a position that is taken is
//! `LedgerError::VersionConflict` with the current head — the same
//! shape the Surreal adapter reports — and the reading of a page is by
//! the keys of its positions, never a scan of the table. (The guarantee
//! L6 of the contract is about the lock the local engine puts on a
//! declared index; here the indexes live in the server, which the
//! service never holds, and the page is read by identity as it is
//! there.)
//!
//! The tenant namespace of the adapter plays the role the namespace and
//! database of the deployment played for the Surreal adapter: one store
//! per deployment. Two tenants that name one stream the same way hold
//! two sequences, never one contended key, because the tenant is in the
//! scope and in the position key.

use std::sync::Arc;

use async_trait::async_trait;
use hodei_evidence::{
    EventEnvelope, EventId, EventLedger, EventPage, ExpectedVersion, IdempotencyKey, LedgerCursor,
    LedgerError, Producer, SchemaVersion,
};
use hodei_graph_client::{
    GraphClient, GuardError, GuardKind, GuardScope, GuardStore, GuardedEffect, OperationId,
    RevisionDemand, Tenant, dql_string,
};
use sha2::{Digest, Sha256};

/// The type tag of ledger event nodes in the graph.
pub const LEDGER_EVENT_TYPE: &str = "HodeiLedgerEvent";

/// The longest stream id the ledger accepts as a scope subject. The
/// guard's own bound; named here so the validation error speaks ledger.
const MAX_STREAM_ID_LEN: usize = 256;

/// The page limit the ledger serves: the same clamp the Surreal adapter
/// carries, so the port cannot be asked for an unbounded query.
const MAX_PAGE: usize = 1_000;

/// Why the ledger could not be opened. The append and read path speaks
/// the port's [`LedgerError`]; only the construction of the adapter
/// fails with this type.
#[derive(Debug, thiserror::Error)]
pub enum DgraphLedgerError {
    /// The tenant namespace is not a valid scope tenant.
    #[error("the ledger tenant '{tenant}' is not valid: {reason}")]
    InvalidTenant {
        /// The tenant as received.
        tenant: String,
        /// Why it was rejected.
        reason: String,
    },
    /// The graph, its schema or its guard refused the store.
    #[error("the ledger could not open on the graph: {0}")]
    Setup(#[from] GuardError),
}

/// The ledger over the graph authority.
///
/// Built on a connected, logged-in client. Opening runs the schema boot
/// (the versioned predicate set including `hodei.ledger.*`) and refuses
/// a server the binary does not know.
#[derive(Debug)]
pub struct DgraphEventLedger {
    guards: GuardStore,
    client: Arc<GraphClient>,
    tenant: Tenant,
}

impl DgraphEventLedger {
    /// Opens the ledger on a connected client for one tenant namespace.
    ///
    /// The setup error carries the guard's closed failure, which is a
    /// large enum on purpose (the stack keeps its detail); construction
    /// happens once per adapter, so the size of the `Result` here is
    /// not on the hot path.
    #[allow(clippy::result_large_err)]
    pub async fn open(client: Arc<GraphClient>, tenant: &str) -> Result<Self, DgraphLedgerError> {
        let tenant = Tenant::new(tenant).map_err(|error| DgraphLedgerError::InvalidTenant {
            tenant: tenant.to_owned(),
            reason: error.to_string(),
        })?;
        let guards = GuardStore::open(client.clone())
            .await
            .map_err(DgraphLedgerError::Setup)?;
        Ok(Self {
            guards,
            client,
            tenant,
        })
    }

    /// The scope that sequences one stream.
    fn scope(&self, stream_id: &str) -> Result<GuardScope, LedgerError> {
        if stream_id.is_empty() {
            return Err(LedgerError::InvalidEvent(
                "the stream id is empty".to_string(),
            ));
        }
        if stream_id.len() > MAX_STREAM_ID_LEN {
            return Err(LedgerError::InvalidEvent(format!(
                "the stream id is longer than {MAX_STREAM_ID_LEN} bytes"
            )));
        }
        // The guard's charset is the HRN-safe subset; a stream id that
        // falls outside it cannot be a conflict key and is refused,
        // never escaped into one.
        GuardScope::new(GuardKind::Ledger, &self.tenant, stream_id)
            .map_err(|error| LedgerError::InvalidEvent(error.to_string()))
    }

    /// The stable digest of a stream under a tenant: the first half of
    /// a position key.
    fn stream_digest(tenant: &str, stream_id: &str) -> String {
        hex::encode(Sha256::digest(format!("{tenant}#{stream_id}").as_bytes()))
    }

    /// The deterministic key of one position: the identity of the event
    /// node, the same role `event_ledger:stream~position` plays in the
    /// local adapter. The tenant is part of the digest: two tenants
    /// naming one stream the same way never contend on one key.
    pub fn position_key(tenant: &str, stream_id: &str, position: u64) -> String {
        format!("{}#{position}", Self::stream_digest(tenant, stream_id))
    }

    /// The operation identity of one delivery: the tenant, the stream,
    /// the position and the event id. Deterministic, so a re-delivery
    /// of the same envelope carries the same id and replays.
    pub fn operation_id(
        tenant: &str,
        stream_id: &str,
        position: u64,
        event_id: &str,
    ) -> OperationId {
        // A hash, not a concatenation: the pieces are caller-chosen
        // strings and the registry key must stay bounded and free of
        // the composite-key separator.
        let digest =
            Sha256::digest(format!("{tenant}#{stream_id}#{position}#{event_id}").as_bytes());
        OperationId::new(&hex::encode(digest)).expect("a sha256 hex is always a valid id")
    }

    /// The head of a stream: its guard revision minus one. An error of
    /// the graph is reported as unavailable — the sequence is unknown,
    /// never guessed.
    async fn head(&self, stream_id: &str) -> Result<Option<u64>, LedgerError> {
        let scope = self.scope(stream_id)?;
        let revision = self
            .guards
            .guard_revision(&scope)
            .await
            .map_err(|error| LedgerError::Unavailable(error.to_string()))?;
        Ok(head_for(revision))
    }

    /// The N-Quads that write one event node, by its position key.
    fn event_nquads(tenant: &str, event: &EventEnvelope) -> String {
        let record = LedgerEventRecord::from_envelope(tenant, event);
        format!(
            "_:evt <dgraph.type> \"{LEDGER_EVENT_TYPE}\" .\n\
             _:evt <hodei.ledger.key> {} .\n\
             _:evt <hodei.ledger.stream_id> {} .\n\
             _:evt <hodei.ledger.position> {} .\n\
             _:evt <hodei.ledger.event_id> {} .\n\
             _:evt <hodei.ledger.tenant_hrn> {} .\n\
             _:evt <hodei.ledger.event_type> {} .\n\
             _:evt <hodei.ledger.schema_version> {} .\n\
             _:evt <hodei.ledger.occurred_at> {} .\n\
             _:evt <hodei.ledger.recorded_at> {} .\n\
             _:evt <hodei.ledger.producer> {} .\n\
             _:evt <hodei.ledger.correlation_id> {} .\n\
             _:evt <hodei.ledger.causation_id> {} .\n\
             _:evt <hodei.ledger.idempotency_key> {} .\n\
             _:evt <hodei.ledger.payload> {} .\n",
            dql_string(&record.key),
            dql_string(&record.stream_id),
            dql_string(&record.position),
            dql_string(&record.event_id),
            dql_string(&record.tenant_hrn),
            dql_string(&record.event_type),
            dql_string(&record.schema_version),
            dql_string(&record.occurred_at),
            dql_string(&record.recorded_at),
            dql_string(&record.producer),
            dql_string(record.correlation_id.as_deref().unwrap_or("")),
            dql_string(record.causation_id.as_deref().unwrap_or("")),
            dql_string(record.idempotency_key.as_deref().unwrap_or("")),
            dql_string(&record.payload),
        )
    }

    /// Reads one position by its key. `None` when the position is not
    /// there: the absence is a response, not an error.
    pub async fn event_at(
        &self,
        stream_id: &str,
        position: u64,
    ) -> Result<Option<LedgerEventRecord>, LedgerError> {
        let key = Self::position_key(self.tenant.as_str(), stream_id, position);
        let nodes = self.fetch_by_keys(std::slice::from_ref(&key)).await?;
        match nodes.len() {
            0 => Ok(None),
            1 => LedgerEventRecord::from_node(&nodes[0])
                .map(Some)
                .map_err(LedgerError::InvalidEvent),
            n => Err(LedgerError::InvalidEvent(format!(
                "{n} event nodes carry the key '{key}'"
            ))),
        }
    }

    /// Fetches the nodes of the given position keys in one round trip.
    /// The answer keeps the order of the keys: the reader pairs each
    /// answer with the position it asked for.
    async fn fetch_by_keys(&self, keys: &[String]) -> Result<Vec<serde_json::Value>, LedgerError> {
        let mut query = String::from("{ ");
        for (index, key) in keys.iter().enumerate() {
            query.push_str(&format!(
                "k{index}(func: eq(hodei.ledger.key, {})) {{ {} }}\n",
                dql_string(key),
                EVENT_FIELDS,
            ));
        }
        query.push('}');
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|error| LedgerError::Unavailable(error.to_string()))?;
        let mut nodes = Vec::with_capacity(keys.len());
        for index in 0..keys.len() {
            nodes.extend(
                response
                    .json
                    .get(format!("k{index}"))
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        Ok(nodes)
    }
}

/// The fields every ledger event node is read with.
const EVENT_FIELDS: &str = "hodei.ledger.key hodei.ledger.stream_id hodei.ledger.position \
     hodei.ledger.event_id hodei.ledger.tenant_hrn hodei.ledger.event_type \
     hodei.ledger.schema_version hodei.ledger.occurred_at hodei.ledger.recorded_at \
     hodei.ledger.producer hodei.ledger.correlation_id hodei.ledger.causation_id \
     hodei.ledger.idempotency_key hodei.ledger.payload";

/// The map from a guard revision to the port's notion of the head.
/// The revision counts committed events, so the head position is the
/// revision minus one, and an empty stream (revision zero) has no head.
pub(crate) fn head_for(revision: u64) -> Option<u64> {
    (revision > 0).then(|| revision - 1)
}

/// The event of the ledger, as the graph stores it: every value is the
/// text the domain gave, so a replay returns the payload byte-semantic,
/// the digest and the scope, and an HRN that was stored parses back to
/// the same account.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LedgerEventRecord {
    /// The identity of the node: `sha256(tenant#stream)#position`.
    pub key: String,
    /// The stream the event belongs to.
    pub stream_id: String,
    /// The position in the stream, as stored text.
    pub position: String,
    /// The identity of the event.
    pub event_id: String,
    /// The account the event belongs to.
    pub tenant_hrn: String,
    /// The type of the event.
    pub event_type: String,
    /// The version of the schema that describes the payload.
    pub schema_version: String,
    /// When the fact happened, RFC 3339.
    pub occurred_at: String,
    /// When the ledger recorded it, RFC 3339.
    pub recorded_at: String,
    /// The producer, as JSON text.
    pub producer: String,
    /// The correlation identity, absent when the event carries none.
    pub correlation_id: Option<String>,
    /// The causation identity, absent when the event carries none.
    pub causation_id: Option<String>,
    /// The key of the delivery, as JSON text; absent when the event
    /// carries none.
    pub idempotency_key: Option<String>,
    /// The payload, as the compact JSON text the domain serialized.
    pub payload: String,
}

impl LedgerEventRecord {
    /// The record of an envelope in a tenant namespace.
    pub fn from_envelope(tenant: &str, event: &EventEnvelope) -> Self {
        let position = event.stream_position();
        Self {
            key: DgraphEventLedger::position_key(tenant, event.stream_id(), position),
            stream_id: event.stream_id().to_string(),
            position: position.to_string(),
            event_id: event.id().as_str().to_string(),
            tenant_hrn: event.tenant_hrn().to_string(),
            event_type: event.event_type().as_str().to_string(),
            schema_version: event.schema_version().to_string(),
            occurred_at: event.occurred_at().to_rfc3339(),
            recorded_at: event.recorded_at().to_rfc3339(),
            producer: serde_json::to_string(event.producer()).unwrap_or_default(),
            correlation_id: event
                .correlation_id()
                .filter(|text| !text.is_empty())
                .map(str::to_string),
            causation_id: event
                .causation_id()
                .map(|id| id.as_str().to_string())
                .filter(|text| !text.is_empty()),
            idempotency_key: event
                .idempotency_key()
                .map(|key| serde_json::to_string(key).unwrap_or_default()),
            payload: serde_json::to_string(event.payload()).unwrap_or_default(),
        }
    }

    /// The envelope this record rebuilds.
    ///
    /// A record that cannot be read back is an error the reader reports,
    /// never a silent skip: a history with holes in it that looks whole
    /// is worse than one that admits them.
    pub fn to_envelope(&self) -> Result<EventEnvelope, String> {
        use hodei_evidence::EventType;

        let tenant = kernel::Hrn::from_string(&self.tenant_hrn)
            .ok_or_else(|| format!("the stored tenant '{}' is not an HRN", self.tenant_hrn))?;
        let position: u64 = self.position.parse().map_err(|error| {
            format!(
                "the stored position '{}' is not a number: {error}",
                self.position
            )
        })?;
        let payload: serde_json::Value = serde_json::from_str(&self.payload)
            .map_err(|error| format!("the stored payload is not JSON: {error}"))?;
        let occurred_at = chrono::DateTime::parse_from_rfc3339(&self.occurred_at)
            .map_err(|error| format!("the stored occurred_at is not RFC 3339: {error}"))?
            .with_timezone(&chrono::Utc);
        let recorded_at = chrono::DateTime::parse_from_rfc3339(&self.recorded_at)
            .map_err(|error| format!("the stored recorded_at is not RFC 3339: {error}"))?
            .with_timezone(&chrono::Utc);

        let producer: Producer = serde_json::from_str(&self.producer)
            .map_err(|error| format!("the stored producer is not JSON: {error}"))?;

        let mut builder = EventEnvelope::builder(
            EventId::new(self.event_id.clone()),
            tenant,
            EventType::from_name(&self.event_type),
        )
        .stream(self.stream_id.clone(), position)
        .occurred_at(occurred_at)
        .recorded_at(recorded_at)
        .producer(producer)
        .payload(payload);

        if let Some(correlation) = &self.correlation_id {
            builder = builder.correlation_id(correlation.clone());
        }
        if let Some(causation) = &self.causation_id {
            builder = builder.causation_id(EventId::new(causation.clone()));
        }
        if let Some(key) = &self.idempotency_key {
            let key: IdempotencyKey = serde_json::from_str(key)
                .map_err(|error| format!("the stored idempotency key is corrupt: {error}"))?;
            builder = builder.idempotency_key(key);
        }
        if let Some((major, minor)) = self.schema_version.split_once('.')
            && let (Ok(major), Ok(minor)) = (major.parse::<u16>(), minor.parse::<u16>())
        {
            builder = builder.schema_version(SchemaVersion::new(major, minor));
        }

        Ok(builder.build())
    }

    /// Reads one stored node (the JSON the server answered) into the
    /// record. A missing field is corruption of the row, not a default.
    fn from_node(node: &serde_json::Value) -> Result<Self, String> {
        let text = |field: &str| -> Result<String, String> {
            node.get(field)
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| format!("the node has no string '{field}'"))
        };
        let optional = |field: &str| -> Option<String> {
            node.get(field)
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };
        Ok(Self {
            key: text("hodei.ledger.key")?,
            stream_id: text("hodei.ledger.stream_id")?,
            position: text("hodei.ledger.position")?,
            event_id: text("hodei.ledger.event_id")?,
            tenant_hrn: text("hodei.ledger.tenant_hrn")?,
            event_type: text("hodei.ledger.event_type")?,
            schema_version: text("hodei.ledger.schema_version")?,
            occurred_at: text("hodei.ledger.occurred_at")?,
            recorded_at: text("hodei.ledger.recorded_at")?,
            producer: text("hodei.ledger.producer")?,
            correlation_id: optional("hodei.ledger.correlation_id"),
            causation_id: optional("hodei.ledger.causation_id"),
            idempotency_key: optional("hodei.ledger.idempotency_key"),
            payload: text("hodei.ledger.payload")?,
        })
    }
}

#[async_trait]
impl EventLedger for DgraphEventLedger {
    /// Appends an event to a stream. Three outcomes, told apart:
    ///
    /// - the position was free and the stream was where the writer says
    ///   it is: the event is written, the guard advances, the operation
    ///   marker commits with them — one atomic fact;
    /// - the same delivery arrives twice (tenant, stream, position and
    ///   event id give the operation its identity): the marker is
    ///   there, the committed event id comes back, nothing is written
    ///   again — even after the stream moved, which is what a lost ACK
    ///   means for the writer;
    /// - another event claims the position: the guard's CAS refuses the
    ///   append with the current head, and that is a version conflict
    ///   the writer has to look at, not retry blindly.
    ///
    /// The demand is the exact revision the position names: the next
    /// free position IS the guard revision, so the CAS and the
    /// position of the event are one check. A writer whose expectation
    /// disagrees with the event it carries (expected one thing, writes
    /// another) loses the same check and is told the head — the same
    /// observable answer the Surreal adapter gives.
    async fn append(
        &self,
        expected: ExpectedVersion,
        event: EventEnvelope,
    ) -> Result<EventId, LedgerError> {
        let stream_id = event.stream_id().to_string();
        let position = event.stream_position();

        // The writer says where it believes the stream is, and the event
        // says where it goes. The two have to agree: a writer that
        // expected one thing and writes at another position is a writer
        // that does not know what it is doing, and the answer is the
        // same the local adapter gives — a conflict with the real head.
        // (When the two agree, the CAS below enforces the expectation
        // against the live stream with the guard's own read.)
        if position != expected.next_position() {
            let current = self.head(&stream_id).await?;
            tracing::warn!(
                stream_id = %stream_id,
                position,
                current = ?current,
                "the event names a position the expectation does not"
            );
            return Err(LedgerError::VersionConflict {
                expected: format!("{expected:?}"),
                actual: current,
            });
        }

        let scope = self.scope(&stream_id)?;
        let operation = Self::operation_id(
            self.tenant.as_str(),
            &stream_id,
            position,
            event.id().as_str(),
        );
        let record = LedgerEventRecord::from_envelope(self.tenant.as_str(), &event);
        let effect = GuardedEffect {
            query_lines: Vec::new(),
            set_nquads: Self::event_nquads(self.tenant.as_str(), &event),
            recorded_result: record.event_id.clone(),
        };

        match self
            .guards
            .execute_guarded(&scope, &operation, RevisionDemand::At(position), effect)
            .await
        {
            Ok(outcome) => {
                // Success is decided by the state after the commit,
                // never inferred: the marker was written in the same
                // transaction as the event node and the guard advance.
                // The replay of a lost ACK carries the id the first
                // attempt committed.
                if outcome.replayed {
                    tracing::info!(
                        stream_id = %stream_id,
                        position,
                        event_id = %outcome.recorded_result,
                        "the delivery was already in the ledger: replayed, not written twice"
                    );
                }
                Ok(EventId::new(outcome.recorded_result))
            }
            Err(GuardError::CasRejected {
                current_revision, ..
            }) => {
                tracing::warn!(
                    stream_id = %stream_id,
                    position,
                    current = current_revision,
                    "the stream moved while the writer was working"
                );
                Err(LedgerError::VersionConflict {
                    expected: format!("{expected:?}"),
                    actual: head_for(current_revision),
                })
            }
            Err(other) => Err(LedgerError::Unavailable(other.to_string())),
        }
    }

    /// Reads a page of events after a cursor.
    ///
    /// The head says where the stream ends; the page covers the
    /// positions the cursor asked for and the head accounts for, and
    /// each is read by its key — one round trip, every root named by
    /// the identity of its position. A position the head accounts for
    /// but that has no node is a hole in the sequence: the guard was
    /// built to make one impossible, so if one ever appears it is
    /// reported as an error, never hidden between two whole events.
    async fn read(&self, cursor: LedgerCursor, limit: usize) -> Result<EventPage, LedgerError> {
        let limit = limit.clamp(1, MAX_PAGE);
        let Some(last) = self.head(&cursor.stream_id).await? else {
            // The stream has no events: the empty page. The absence is
            // a response, never an error, and never confused with the
            // error of a stream whose sequence is broken (below).
            return Ok(EventPage {
                events: Vec::new(),
                next_cursor: None,
            });
        };
        if cursor.next_position > last {
            return Ok(EventPage {
                events: Vec::new(),
                next_cursor: None,
            });
        }

        let first = cursor.next_position;
        let end = last.min(first + limit as u64 - 1);
        let keys: Vec<String> = (first..=end)
            .map(|position| {
                DgraphEventLedger::position_key(self.tenant.as_str(), &cursor.stream_id, position)
            })
            .collect();

        let nodes = self.fetch_by_keys(&keys).await?;
        if nodes.len() != keys.len() {
            // The head counted an event the graph does not have.
            return Err(LedgerError::InvalidEvent(format!(
                "the stream '{}' accounts for {} positions but the graph holds {}: \
                 the sequence has a hole",
                cursor.stream_id,
                keys.len(),
                nodes.len(),
            )));
        }

        let mut events = Vec::with_capacity(nodes.len());
        for node in &nodes {
            let record = LedgerEventRecord::from_node(node).map_err(LedgerError::InvalidEvent)?;
            events.push(record.to_envelope().map_err(LedgerError::InvalidEvent)?);
        }

        let next_cursor = if end < last {
            Some(LedgerCursor::from_position(
                cursor.stream_id.clone(),
                end + 1,
            ))
        } else {
            None
        };

        Ok(EventPage {
            events,
            next_cursor,
        })
    }

    /// Where a stream is: the last written position, or nothing when
    /// the stream has no events. The head is the guard revision.
    async fn position(&self, stream_id: &str) -> Result<Option<u64>, LedgerError> {
        self.head(stream_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(id: &str, stream: &str, position: u64) -> EventEnvelope {
        let tenant =
            kernel::Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0")
                .expect("an HRN");
        EventEnvelope::builder(
            EventId::new(id),
            tenant,
            hodei_evidence::EventType::EvidenceAccepted,
        )
        .stream(stream, position)
        .occurred_at(
            chrono::DateTime::parse_from_rfc3339("2026-01-01T10:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        )
        .recorded_at(
            chrono::DateTime::parse_from_rfc3339("2026-01-01T10:00:20Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        )
        .producer(Producer::new("connector", "pipelinek"))
        .idempotency_key(IdempotencyKey::derive(
            &Producer::new("connector", "pipelinek"),
            &hodei_evidence::SourceRef::by_external_id(format!("run-{id}")),
            &hodei_evidence::ContentDigest::sha256("a".repeat(64)),
            "1",
        ))
        .payload(serde_json::json!({"evidence_id": id, "note": "quote\" and \\ backslash"}))
        .build()
    }

    /// The head map: revision 0 is an empty stream; the revision counts
    /// committed events, so the head position is one below.
    #[test]
    fn the_revision_maps_to_the_head() {
        assert_eq!(head_for(0), None);
        assert_eq!(head_for(1), Some(0));
        assert_eq!(head_for(42), Some(41));
    }

    /// The position key is deterministic and tenant-scoped: two tenants
    /// naming one stream the same way never contend on one key.
    #[test]
    fn the_position_key_is_stable_and_scoped() {
        let first = DgraphEventLedger::position_key("acme", "stream-1", 7);
        let second = DgraphEventLedger::position_key("acme", "stream-1", 7);
        assert_eq!(first, second, "the key is derived, not generated");
        assert_ne!(
            first,
            DgraphEventLedger::position_key("acme", "stream-1", 8),
            "one position is one key"
        );
        assert_ne!(
            first,
            DgraphEventLedger::position_key("other", "stream-1", 7),
            "two tenants never share a key"
        );
        assert!(!first.contains('|'), "the key survives the DQL literal");
        assert!(first.len() <= 85, "the key is bounded: {first}");
    }

    /// The operation identity is the delivery: the same envelope gives
    /// the same id; a different event, position, stream or tenant gives
    /// a different one.
    #[test]
    fn the_operation_identity_is_the_delivery() {
        let a = DgraphEventLedger::operation_id("acme", "stream-1", 0, "evt-1");
        let same = DgraphEventLedger::operation_id("acme", "stream-1", 0, "evt-1");
        assert_eq!(
            a.as_str(),
            same.as_str(),
            "a re-delivery is the same operation"
        );
        assert_ne!(
            a.as_str(),
            DgraphEventLedger::operation_id("acme", "stream-1", 0, "evt-2").as_str(),
            "another event is another operation"
        );
        assert_ne!(
            a.as_str(),
            DgraphEventLedger::operation_id("acme", "stream-2", 0, "evt-1").as_str(),
            "another stream is another operation"
        );
        assert_ne!(
            a.as_str(),
            DgraphEventLedger::operation_id("other", "stream-1", 0, "evt-1").as_str(),
            "another tenant is another operation"
        );
    }

    /// The record round-trips an envelope: the payload is the same JSON,
    /// the timestamps keep their instant, the scope and the delivery key
    /// survive the store — AC#2 of the replay.
    #[test]
    fn the_record_round_trips_the_envelope() {
        let original = envelope("evt-1", "stream-1", 3);
        let record = LedgerEventRecord::from_envelope("acme", &original);
        assert_eq!(record.position, "3");
        assert_eq!(record.event_id, "evt-1");
        let rebuilt = record.to_envelope().expect("the record reads back");
        assert_eq!(
            rebuilt.payload(),
            original.payload(),
            "the payload is byte-semantic: the same JSON"
        );
        assert_eq!(rebuilt.id(), original.id());
        assert_eq!(rebuilt.stream_position(), 3);
        assert_eq!(rebuilt.tenant_hrn(), original.tenant_hrn());
        assert_eq!(rebuilt.occurred_at(), original.occurred_at());
        assert_eq!(rebuilt.recorded_at(), original.recorded_at());
        assert_eq!(rebuilt.event_type(), original.event_type());
        assert_eq!(rebuilt.producer(), original.producer());
        assert_eq!(
            rebuilt.idempotency_key(),
            original.idempotency_key(),
            "the digest and the delivery identity survive the round trip"
        );
    }

    /// The effect's N-Quads escape the payload: a quote or a backslash
    /// in the bytes cannot break out of the DQL literal, and every quad
    /// writes the one event node.
    #[test]
    fn the_nquads_escape_the_payload() {
        let original = envelope("evt-1", "stream-1", 0);
        let nquads = DgraphEventLedger::event_nquads("acme", &original);
        // The payload carries a quote and a backslash; in the stored
        // literal the backslash is doubled and the quote is escaped.
        assert!(nquads.contains(r#"\\\"#), "the backslash is doubled");
        assert!(nquads.contains(r#"\""#), "the quote is escaped");
        for line in nquads.lines() {
            assert!(
                line.starts_with("_:evt "),
                "every quad writes the event node"
            );
            assert!(line.ends_with(" ."), "every quad is closed");
        }
        assert!(nquads.contains(&format!(
            "_:evt <hodei.ledger.key> {} .",
            dql_string(&DgraphEventLedger::position_key("acme", "stream-1", 0))
        )));
    }

    /// The absent optionals are stored absent: an empty string is not
    /// read back as a present identity.
    #[test]
    fn the_absent_optionals_stay_absent() {
        let tenant =
            kernel::Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0")
                .expect("an HRN");
        let plain = EventEnvelope::builder(
            EventId::new("evt-plain"),
            tenant,
            hodei_evidence::EventType::EvidenceAccepted,
        )
        .stream("stream-1", 0)
        .payload(serde_json::json!({}))
        .build();
        let record = LedgerEventRecord::from_envelope("acme", &plain);
        assert_eq!(record.correlation_id, None);
        assert_eq!(record.causation_id, None);
        assert_eq!(record.idempotency_key, None);
        let rebuilt = record.to_envelope().expect("the plain record reads back");
        assert_eq!(rebuilt.correlation_id(), None);
        assert_eq!(rebuilt.idempotency_key(), None);
    }

    /// A record whose stored text is not what the contract says is
    /// corruption, reported as an error — the reader never rebuilds a
    /// hole as a whole event.
    #[test]
    fn a_corrupt_record_is_refused_not_skipped() {
        let mut record =
            LedgerEventRecord::from_envelope("acme", &envelope("evt-1", "stream-1", 0));
        record.payload = "{ not json".to_string();
        assert!(record.to_envelope().is_err());
        let mut record =
            LedgerEventRecord::from_envelope("acme", &envelope("evt-1", "stream-1", 0));
        record.tenant_hrn = "not-an-hrn".to_string();
        assert!(record.to_envelope().is_err());
        let mut record =
            LedgerEventRecord::from_envelope("acme", &envelope("evt-1", "stream-1", 0));
        record.idempotency_key = Some("{ broken".to_string());
        assert!(record.to_envelope().is_err());
    }
}
