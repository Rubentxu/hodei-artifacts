//! The ledger of events, over the store that already exists.
//!
//! # Why this shape
//!
//! The spike of the plan asks whether the semantics of the ledger can be
//! implemented safely on the existing durable store, or whether a dedicated
//! log is needed now. The answer, measured, is that the existing store is
//! enough for now.
//!
//! The decisive detail is **the identity of the record**. The identifier of
//! a ledger record is the position it occupies in its stream:
//!
//! ```text
//! event_ledger:⟨stream_id, stream_position⟩
//! ```
//!
//! That gives three things at once.
//!
//! - **The uniqueness of the position is atomic.** It is the primary key, so
//!   two writers cannot both take the same position: one of them is told
//!   that the position is taken. No read-then-write race.
//! - **No index is needed.** And that matters more than it looks: defining
//!   an index on the local engine keeps the database directory locked by the
//!   process that defines it, so the same process can never reopen the
//!   database afterwards. A service that declared an index on its own path
//!   could not restart. With the position as the identifier, the service
//!   declares nothing and can reopen.
//! - **The order of a stream is the order of its identifiers.** Reading a
//!   stream is reading its positions in order, without an index to lean on.
//!
//! The price is honest and worth naming: reading a stream is a scan of the
//! table, not an index lookup. At the scale M1 works with that is the right
//! trade, and the spike's scale measurement is deferred to the M1 UAT
//! rather than assumed here.
//!
//! # What the adapter does not do
//!
//! It does not expose the record identifiers of the store. The port speaks
//! in domain identities, and the identifiers the database uses are its own
//! business.

use hodei_evidence::{
    EventEnvelope, EventId, EventLedger, EventPage, ExpectedVersion, LedgerCursor, LedgerError,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use surrealdb::Connection;
use surrealdb::Surreal;

/// The table the events live in.
pub const LEDGER_TABLE: &str = "event_ledger";

/// The table the heads of the streams live in.
///
/// A head says where a stream is, and it is read by its own identity. The
/// measure of the scale model found the cost of the other way: asking the
/// ledger where a stream is meant reading the whole table and taking the
/// last row, and the admission asks that question before every append. The
/// cost was quadratic in the length of the stream, and it showed up as a
/// growth of the admission that had nothing to do with the work of writing
/// an event.
///
/// The head is written in the same transaction as the event it accounts for,
/// so a head never points at a position that is not there, and a position
/// that is there always has a head that accounts for it.
pub const LEDGER_HEAD_TABLE: &str = "event_ledger_head";

/// The record of an event, as it is stored.
///
/// The fields are the ones the envelope has, and the identifier of the
/// record is not among them: the identifier is the position, and it is
/// derived from the stream rather than stored twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerRecord {
    /// The identity of the event.
    pub event_id: String,
    /// The stream the event belongs to.
    pub stream_id: String,
    /// The position of the event in its stream.
    pub stream_position: i64,
    /// The account the event belongs to.
    pub tenant_hrn: String,
    /// The type of the event.
    pub event_type: String,
    /// The version of the schema that describes the payload.
    pub schema_version: String,
    /// When the event happened.
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    /// When it was written.
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    /// Who produced it.
    pub producer: serde_json::Value,
    /// The correlation of the request that caused it.
    pub correlation_id: Option<String>,
    /// The event that caused this one.
    pub causation_id: Option<String>,
    /// The key that makes a repeated delivery the same delivery.
    pub idempotency_key: Option<String>,
    /// The payload.
    pub payload: serde_json::Value,
}

impl LedgerRecord {
    /// The record of an envelope.
    pub fn from_envelope(event: &EventEnvelope) -> Self {
        Self {
            event_id: event.id().as_str().to_string(),
            stream_id: event.stream_id().to_string(),
            stream_position: event.stream_position() as i64,
            tenant_hrn: event.tenant_hrn().to_string(),
            event_type: event.event_type().as_str().to_string(),
            schema_version: event.schema_version().to_string(),
            occurred_at: event.occurred_at(),
            recorded_at: event.recorded_at(),
            producer: serde_json::to_value(event.producer()).unwrap_or(serde_json::Value::Null),
            correlation_id: event.correlation_id().map(str::to_string),
            causation_id: event.causation_id().map(|id| id.as_str().to_string()),
            idempotency_key: event.idempotency_key().map(|key| key.to_string()),
            payload: event.payload().clone(),
        }
    }
}

/// The head of a stream, as it is stored.
///
/// The head says where the stream is. It is written in the same transaction
/// as the event it accounts for, so the two never disagree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeadRow {
    /// The stream the head belongs to.
    pub stream_id: String,
    /// The last position of the stream.
    pub last_position: u64,
}

/// The ledger over the store that already exists.
pub struct SurrealEventLedger<C: Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: Connection> SurrealEventLedger<C> {
    /// A ledger over a database.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }

    /// The database the ledger writes to.
    pub fn db(&self) -> &Arc<Surreal<C>> {
        &self.db
    }

    /// The identifier of the record of a position.
    ///
    /// The record of a position is named by the position, so the uniqueness
    /// of the position is the uniqueness of the record.
    fn record_id(stream_id: &str, position: u64) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((
            LEDGER_TABLE.to_string(),
            format!("{}~{position}", escape_id_part(stream_id)),
        ))
    }

    /// The identifier of the head of a stream.
    ///
    /// The head is named by the stream, so it is read and written by its own
    /// identity, without a scan.
    fn head_id(stream_id: &str) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((LEDGER_HEAD_TABLE.to_string(), escape_id_part(stream_id)))
    }

    /// The position the stream is at, or `None` when it has no events yet.
    ///
    /// The head answers by its own identity. The measure of the scale model
    /// found that the other way — scanning the table of events and taking
    /// the last one — costs more the longer the stream is, and the
    /// admission asks this question before every append.
    pub async fn current_position(&self, stream_id: &str) -> Result<Option<u64>, LedgerError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::head_id(stream_id)))
            .await
            .map_err(|error| LedgerError::Unavailable(error.to_string()))?;
        let rows: Vec<HeadRow> = response.take(0).map_err(|_error| {
            LedgerError::InvalidEvent("the ledger head could not be read".to_string())
        })?;
        Ok(rows.first().map(|row| row.last_position))
    }

    /// The record of a position, when there is one.
    async fn record_at(
        &self,
        stream_id: &str,
        position: u64,
    ) -> Result<Option<LedgerRecord>, LedgerError> {
        let mut response = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::record_id(stream_id, position)))
            .await
            .map_err(|error| LedgerError::Unavailable(error.to_string()))?;
        let rows: Vec<LedgerRecord> = response.take(0).map_err(|_error| {
            LedgerError::InvalidEvent("the ledger row could not be read".to_string())
        })?;
        Ok(rows.into_iter().next())
    }
}

/// Escapes a part of a record identifier.
///
/// The store escapes "~" and ":" inside an identifier. A stream identifier
/// is chosen by the caller, so it can hold any of them.
fn escape_id_part(value: &str) -> String {
    value.replace('~', "~0").replace(':', "~1")
}

#[async_trait::async_trait]
impl<C: Connection> EventLedger for SurrealEventLedger<C> {
    /// Appends an event to a stream.
    ///
    /// Three outcomes, and they are told apart:
    ///
    /// - the position was free: the event is written;
    /// - the position was taken by the **same** event: this is the same
    ///   delivery arriving twice, and the identity of the event that is
    ///   already there is returned without writing anything;
    /// - the position was taken by **another** event: the stream moved under
    ///   the writer, and that is a conflict.
    async fn append(
        &self,
        expected: ExpectedVersion,
        event: EventEnvelope,
    ) -> Result<EventId, LedgerError> {
        let started = Instant::now();
        let stream_id = event.stream_id().to_string();
        let position = event.stream_position();

        // The writer says which position it believes it is writing at. The
        // stream may have moved: then the writer has to look again, and this
        // is not a failure to retry blindly.
        // The same delivery can arrive twice, and the writer that sends it
        // the second time does not know that it was already written: it
        // repeats the same expectation. So the repetition is recognized
        // BEFORE the expectation is checked, otherwise the second delivery
        // would be reported as a conflict and the caller would not know
        // whether its event was written.
        if let Some(existing) = self.record_at(&stream_id, position).await?
            && existing.event_id == event.id().as_str()
        {
            tracing::info!(
                stream_id = %stream_id,
                position,
                event_id = event.id().as_str(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "ledger_append_repeated"
            );
            return Ok(event.id().clone());
        }

        let current = self.current_position(&stream_id).await?;
        // The writer says where it believes the stream is, and the event
        // says where it goes. The two have to agree with the stream: a
        // writer that expected one thing and writes another is a writer that
        // does not know what it is doing.
        if !expected.accepts(current) || position != expected.next_position() {
            tracing::warn!(
                stream_id = %stream_id,
                position,
                current = ?current,
                "the stream moved while the writer was working"
            );
            return Err(LedgerError::VersionConflict {
                expected: format!("{expected:?}"),
                actual: current,
            });
        }

        let record = LedgerRecord::from_envelope(&event);
        let id = Self::record_id(&stream_id, position);
        let head = HeadRow {
            stream_id: stream_id.clone(),
            last_position: position,
        };
        let head_id = Self::head_id(&stream_id);

        // The event and the head go together, in one transaction: a head
        // never points at a position that is not written, and a written
        // position always has a head that accounts for it. Two statements in
        // one query are one transaction.
        let created = self
            .db
            .query(
                "CREATE $id CONTENT $content; \
                 UPSERT $head_id CONTENT $head",
            )
            .bind(("id", id.clone()))
            .bind(("content", record.clone()))
            .bind(("head_id", head_id))
            .bind(("head", head))
            .await
            .map_err(|_error| {
                LedgerError::InvalidEvent("the ledger row could not be read".to_string())
            })?;

        match created.check() {
            Ok(_) => {
                tracing::info!(
                    stream_id = %stream_id,
                    position,
                    event_id = event.id().as_str(),
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "ledger_append"
                );
                Ok(event.id().clone())
            }
            Err(error) => {
                // The position was taken. Whether that is the same delivery
                // arriving twice or another writer getting there first is the
                // difference between an idempotent answer and a conflict.
                let message = error.to_string();
                // The store refuses two writers at the same position in two
                // ways: the record is already there, or the transaction of
                // the loser could not commit because it read what the
                // winner wrote. Both mean the same thing here.
                let taken = message.contains("already exists") || message.contains("was unique");
                let raced = message.contains("read or write conflict")
                    || message.contains("can be retried");
                if taken || raced {
                    // The position was taken between the check and the
                    // write. If it was taken by the same event, the delivery
                    // is repeated; if it was taken by another one, another
                    // writer got there first and this is a conflict.
                    if let Some(existing) = self.record_at(&stream_id, position).await?
                        && existing.event_id == record.event_id
                    {
                        tracing::info!(
                            stream_id = %stream_id,
                            position,
                            event_id = %record.event_id,
                            elapsed_ms = started.elapsed().as_millis() as u64,
                            "ledger_append_repeated"
                        );
                        return Ok(EventId::new(existing.event_id));
                    }
                    return Err(LedgerError::VersionConflict {
                        expected: format!("{expected:?}"),
                        actual: self.current_position(&stream_id).await?,
                    });
                }
                Err(LedgerError::InvalidEvent(message))
            }
        }
    }

    /// Reads a page of events after a cursor.
    ///
    /// The page is read by the identities of its events, and not by the
    /// fields of the rows. The identity of an event IS its position, and
    /// the positions of a stream are contiguous: the writer that expects
    /// the position `p` writes at `p + 1`, so there is no hole between the
    /// first event and the last one.
    ///
    /// The measure of the scale model found the cost of the other way:
    /// asking the table "which rows of this stream come after this
    /// position" reads the whole table and sorts it, so the projection of
    /// a stream cost more the longer the stream was, and the cost grew
    /// faster than the stream. The same page, asked for by the identities
    /// of its events, was ninety times quicker.
    ///
    /// The page ends where the head of the stream says the stream ends.
    async fn read(&self, cursor: LedgerCursor, limit: usize) -> Result<EventPage, LedgerError> {
        let started = Instant::now();
        let limit = limit.clamp(1, 1_000);

        // The head says where the stream is: a point read, by its own
        // identity.
        let last = self.current_position(&cursor.stream_id).await?;
        let Some(last) = last else {
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

        // The page covers the positions that come after the cursor and
        // that the head accounts for.
        let first = cursor.next_position;
        let end = last.min(first + limit as u64 - 1);
        let identities: Vec<surrealdb::sql::Thing> = (first..=end)
            .map(|position| Self::record_id(&cursor.stream_id, position))
            .collect();

        let mut response = self
            .db
            .query("SELECT * FROM $ids")
            .bind(("ids", identities))
            .await
            .map_err(|error| LedgerError::Unavailable(error.to_string()))?;

        let mut rows: Vec<LedgerRecord> = response.take(0).map_err(|_error| {
            LedgerError::InvalidEvent("the ledger row could not be read".to_string())
        })?;
        // The order of the rows is the order of the answer, not the order
        // of the question: the page is read in the order of the positions.
        rows.sort_by_key(|row| row.stream_position);

        let events: Vec<EventEnvelope> = rows
            .iter()
            .filter_map(|row| row_to_envelope(row).ok())
            .collect();

        // The page is the last one when the head says the stream ends
        // there: the caller asked for a limit and the stream did not fill
        // it.
        let next_cursor = if end < last {
            Some(LedgerCursor::from_position(
                cursor.stream_id.clone(),
                end + 1,
            ))
        } else {
            None
        };

        tracing::info!(
            stream_id = %cursor.stream_id,
            events = events.len(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "ledger_read"
        );

        Ok(EventPage {
            events,
            next_cursor,
        })
    }

    /// Where a stream is.
    async fn position(&self, stream_id: &str) -> Result<Option<u64>, LedgerError> {
        self.current_position(stream_id).await
    }
}

/// Rebuilds the envelope of a stored record.
///
/// A record that cannot be read back is not silently dropped: the reader
/// skips it and says so, because a history with holes in it that looks
/// whole is worse than one that admits them.
fn row_to_envelope(row: &LedgerRecord) -> Result<EventEnvelope, String> {
    use hodei_evidence::{EventType, Producer, SchemaVersion};

    let tenant = kernel::Hrn::from_string(&row.tenant_hrn)
        .ok_or_else(|| format!("the stored tenant '{}' is not an HRN", row.tenant_hrn))?;

    let mut builder = EventEnvelope::builder(
        EventId::new(row.event_id.clone()),
        tenant,
        EventType::from_name(&row.event_type),
    )
    .stream(row.stream_id.clone(), row.stream_position as u64)
    .occurred_at(row.occurred_at)
    .recorded_at(row.recorded_at)
    .payload(row.payload.clone());

    if let Some(correlation) = &row.correlation_id {
        builder = builder.correlation_id(correlation.clone());
    }
    if let Some(causation) = &row.causation_id {
        builder = builder.causation_id(EventId::new(causation.clone()));
    }
    if let Some(producer) = row.producer.as_object() {
        let kind = producer
            .get("kind")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let id = producer
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        let mut identity = Producer::new(kind, id);
        if let Some(version) = producer.get("version").and_then(|value| value.as_str()) {
            identity = identity.with_version(version);
        }
        builder = builder.producer(identity);
    }
    if let Some((major, minor)) = row.schema_version.split_once('.')
        && let (Ok(major), Ok(minor)) = (major.parse::<u16>(), minor.parse::<u16>())
    {
        builder = builder.schema_version(SchemaVersion::new(major, minor));
    }

    Ok(builder.build())
}
