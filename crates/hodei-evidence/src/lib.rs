//! The evidence and the event ledger of Hodei Artifacts.
//!
//! This context holds two things that the rest of the product builds on:
//!
//! - **The evidence**: why Hodei believes something.
//!
//!   A piece of evidence is an observation made by somebody else -- an SBOM,
//!   an attestation, a build log -- and the record of it says what it is,
//!   who produced it, what its content digest is and how far its
//!   verification goes.
//!
//! - **The event ledger**: what Hodei accepted and changed, in order.
//!
//!   The graph is a projection of the ledger, and the projection can be
//!   thrown away and rebuilt. The ledger cannot.
//!
//! # The rule that shapes both
//!
//! The state of the graph is replaceable; the accepted evidence and the
//! security-relevant history are not.
//!
//! # What is deliberately not here
//!
//! There is no adapter, no store and no HTTP in this crate. The contracts
//! are domain contracts, and a domain contract that mentions a record
//! identifier of a database is already a compromise. The adapters arrive
//! later, behind these ports.
//!
//! There is also no raw payload. A record *references* its content by
//! digest and location; it does not carry it. That is what makes the
//! handling of secrets structural instead of a promise: a record of
//! evidence has no field where a secret would go.

pub mod application;
pub mod domain;
pub mod ports;

pub use application::ingestion::{EvidenceIngestion, IngestionError, IngestionRequest};
pub use application::projector::EvidenceGraphProjector;
pub use domain::content::{ContentDigest, ContentError, ContentRef, DigestAlgorithm};
pub use domain::event::{
    EventEnvelope, EventEnvelopeBuilder, EventError, EventId, EventType, EvidencePayload,
};
pub use domain::evidence::{
    EvidenceError, EvidenceId, EvidenceRecord, EvidenceRecordBuilder, EvidenceType,
    EvidenceVerification, Normalization, Producer, SourceRef, VerificationStatus,
};
pub use domain::graph::{
    GraphChangeSet, GraphNode, GraphRelation, NodeKind, RelationClaim, RelationType,
};
pub use domain::idempotency::{IdempotencyKey, Redelivery};
pub use domain::ingestion::{IngestionEnvelope, IngestionOutcome, RejectionReason};
pub use domain::version::{SchemaVersion, Upcaster, Upcasters, VersionError};
pub use ports::graph::{GraphError, GraphProjector, GraphQuery, GraphWriter, Lineage};
pub use ports::{
    AuthenticationError, ContentStore, DeliveryGuarantee, EnvelopeAuthenticator, EventLedger,
    EventPage, EvidenceStore, ExpectedVersion, LedgerCursor, LedgerError, StoreError,
};
