//! The ports of the evidence context.
//!
//! The contracts are the ones of the plan, and they are contracts of the
//! domain, not of a store: nothing here mentions a record identifier of a
//! database, a transaction, or a table. An adapter is free to be a database,
//! a file, or an in-memory map, and the domain does not find out.
//!
//! The delivery semantics are **at least once, with idempotent consumers**.
//! Saying "exactly once" would be a claim that has to be proven end to end,
//! including the side effects, and nothing here proves it. What the
//! contract does guarantee is that a repeated delivery is recognizable: the
//! idempotency key is part of the event, so a consumer can tell a repeat
//! from a new fact.

use crate::domain::event::{EventEnvelope, EventId};
use crate::domain::evidence::{EvidenceId, EvidenceRecord};
use async_trait::async_trait;
use std::fmt;

/// The store of accepted evidence.
#[async_trait]
pub trait EvidenceStore: Send + Sync {
    /// Stores an accepted piece of evidence and returns its identity.
    ///
    /// Storing the same record twice returns the same identity: the store
    /// is idempotent for the same record.
    async fn put(&self, evidence: EvidenceRecord) -> Result<EvidenceId, StoreError>;

    /// Reads a piece of evidence by its identity.
    ///
    /// It returns nothing when there is no such record. Absence is not an
    /// error: the caller asked whether it is there.
    async fn get(&self, id: &EvidenceId) -> Result<Option<EvidenceRecord>, StoreError>;
}

/// The position the ledger expects a stream to be at.
///
/// This is the optimistic concurrency of the ledger: the writer says where it
/// believes the stream is, and the ledger refuses when the stream moved. It
/// is what lets two writers work without a global lock.
///
/// The position that was expected is the **last written** one, so the event
/// that follows goes to the next position. A stream that has no events yet
/// is `Absent`, and the first event goes to position zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedVersion {
    /// The last written position is this one.
    At(u64),
    /// The stream has no events yet.
    Absent,
}

impl ExpectedVersion {
    /// Whether the given position is the one that was expected.
    pub fn accepts(&self, current: Option<u64>) -> bool {
        match self {
            Self::At(position) => current == Some(*position),
            Self::Absent => current.is_none(),
        }
    }

    /// The position the next event occupies when the expectation holds.
    pub fn next_position(&self) -> u64 {
        match self {
            Self::At(position) => position + 1,
            Self::Absent => 0,
        }
    }
}

/// The error of an append that did not happen.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LedgerError {
    /// The stream moved while the writer was working.
    #[error("the stream is at version {actual:?} and the writer expected {expected:?}")]
    VersionConflict {
        /// What the writer expected.
        expected: String,
        /// Where the stream really is.
        actual: Option<u64>,
    },
    /// The event was already written.
    #[error("the event was already recorded")]
    Duplicate,
    /// The event cannot be written as it is.
    #[error("the event cannot be recorded: {0}")]
    InvalidEvent(String),
    /// The store is not available.
    #[error("the ledger is not available")]
    Unavailable,
}

/// The cursor of a read over the ledger.
///
/// The cursor is a position in the history, not an offset into a table. A
/// consumer that stops and resumes with the same cursor continues where it
/// left, and it does not miss or repeat an event because the ledger grew.
///
/// The position is the one of the **next event to read**, so the cursor at
/// the beginning of a stream is position zero and the first event of the
/// stream is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerCursor {
    /// The stream the cursor is in.
    pub stream_id: String,
    /// The position of the next event to read.
    pub next_position: u64,
}

impl LedgerCursor {
    /// A cursor at the beginning of a stream.
    pub fn start_of(stream_id: impl Into<String>) -> Self {
        Self {
            stream_id: stream_id.into(),
            next_position: 0,
        }
    }

    /// A cursor that continues at a position.
    pub fn from_position(stream_id: impl Into<String>, position: u64) -> Self {
        Self {
            stream_id: stream_id.into(),
            next_position: position,
        }
    }
}

/// A page of events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventPage {
    /// The events, in order.
    pub events: Vec<EventEnvelope>,
    /// The cursor to resume from, when there are more.
    pub next_cursor: Option<LedgerCursor>,
}

impl EventPage {
    /// Whether the page is the last one.
    pub fn is_last(&self) -> bool {
        self.next_cursor.is_none()
    }
}

/// The ledger of events.
///
/// The ledger is the durable history: what Hodei accepted and changed, in
/// order. The graph is a projection of it, and the projection can be thrown
/// away and rebuilt. This cannot.
#[async_trait]
pub trait EventLedger: Send + Sync {
    /// Appends an event to a stream.
    ///
    /// The writer says which position it believes it is writing at, and the
    /// ledger refuses when the stream moved. The refusal is not a failure to
    /// retry blindly: the writer has to look again.
    async fn append(
        &self,
        expected: ExpectedVersion,
        event: EventEnvelope,
    ) -> Result<EventId, LedgerError>;

    /// Reads a page of events after a cursor.
    async fn read(&self, cursor: LedgerCursor, limit: usize) -> Result<EventPage, LedgerError>;
}

/// The error of a store that cannot do its job.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The store is not available.
    #[error("the evidence store is not available")]
    Unavailable,
    /// The record cannot be stored as it is.
    #[error("the evidence cannot be stored: {0}")]
    InvalidRecord(String),
}

/// The delivery guarantee of the ledger.
///
/// The plan is explicit: at least once, with idempotent consumers, unless a
/// store can give something stronger without coupling the domain to it. The
/// domain does not know which one it has, so it states the weaker one and
/// makes the consumer idempotent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryGuarantee {
    /// The event can arrive more than once, and the consumer has to
    /// recognize the repeat.
    AtLeastOnceWithIdempotentConsumers,
}

impl fmt::Display for DeliveryGuarantee {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("at least once, with idempotent consumers")
    }
}
