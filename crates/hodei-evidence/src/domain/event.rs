//! The envelope of an event in the ledger.
//!
//! The ledger preserves what Hodei accepted and changed, in order. An event
//! is the unit of that history, and it is immutable: once written, what
//! happened is what happened. A correction is another event, not an edit.
//!
//! The envelope carries the identity of the event, the stream it belongs to
//! and its position in it, the type of what happened, the version of the
//! schema that describes it, when it happened and when it was recorded, who
//! produced it, the correlation and the causation that tie it to the rest of
//! the history, the idempotency key that makes a repeated delivery the same
//! delivery, and the payload.
//!
//! The payload is opaque on purpose. The envelope describes the *shape* of
//! the history; what an event of a given type carries is described by that
//! type's schema, and the version says which one.

use crate::domain::evidence::{EvidenceId, Producer};
use crate::domain::idempotency::IdempotencyKey;
use crate::domain::version::SchemaVersion;
use chrono::{DateTime, Utc};
use kernel::Hrn;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// The identity of an event.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EventId(String);

impl EventId {
    /// The identity of an event.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The identity as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// What an event says happened.
///
/// The categories are the ones of the plan. Adding one is a compatible
/// addition; changing the meaning of one is not, and that is what a new
/// event type and a new schema version are for.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EventType {
    /// Evidence arrived.
    EvidenceReceived,
    /// Evidence was refused, and nothing was accepted.
    EvidenceRejected,
    /// Evidence was accepted.
    EvidenceAccepted,
    /// Evidence was superseded by another.
    EvidenceSuperseded,
    /// The verification of evidence changed.
    EvidenceVerificationChanged,

    /// A projection was applied.
    ProjectionApplied,
    /// A projection failed.
    ProjectionFailed,
    /// A projection checkpointed.
    ProjectionCheckpointed,
    /// A projection rebuild started.
    ProjectionRebuildStarted,
    /// A projection rebuild finished.
    ProjectionRebuildCompleted,

    /// A trust evaluation was requested.
    TrustEvaluationRequested,
    /// A trust decision was created.
    TrustDecisionCreated,
    /// A trust decision was superseded.
    TrustDecisionSuperseded,
    /// A policy version was activated.
    PolicyVersionActivated,

    /// A security case was opened.
    SecurityCaseOpened,
    /// A security case was updated.
    SecurityCaseUpdated,
    /// The verification of a security case was requested.
    SecurityCaseVerificationRequested,
    /// A security case was verified as fixed.
    SecurityCaseVerifiedFixed,

    /// A behavior was scheduled.
    BehaviorScheduled,
    /// A behavior started.
    BehaviorStarted,
    /// A behavior finished.
    BehaviorCompleted,
    /// A behavior failed.
    BehaviorFailed,
    /// A behavior asked for a side effect.
    BehaviorSideEffectRequested,
    /// The side effect was carried out.
    BehaviorSideEffectCompleted,

    /// An event this version does not know.
    Other(String),
}

impl EventType {
    /// The type of an event, from the name it is written with.
    ///
    /// A name that this version does not know is not an error: it is an
    /// event of a type that came later, and the history has to be able to
    /// hold it. That is what `Other` is for.
    pub fn from_name(name: &str) -> Self {
        match name {
            "evidence.received" => Self::EvidenceReceived,
            "evidence.rejected" => Self::EvidenceRejected,
            "evidence.accepted" => Self::EvidenceAccepted,
            "evidence.superseded" => Self::EvidenceSuperseded,
            "evidence.verification_changed" => Self::EvidenceVerificationChanged,
            "projection.applied" => Self::ProjectionApplied,
            "projection.failed" => Self::ProjectionFailed,
            "projection.checkpointed" => Self::ProjectionCheckpointed,
            "projection.rebuild_started" => Self::ProjectionRebuildStarted,
            "projection.rebuild_completed" => Self::ProjectionRebuildCompleted,
            "trust.evaluation_requested" => Self::TrustEvaluationRequested,
            "trust.decision_created" => Self::TrustDecisionCreated,
            "trust.decision_superseded" => Self::TrustDecisionSuperseded,
            "policy.version_activated" => Self::PolicyVersionActivated,
            "security_case.opened" => Self::SecurityCaseOpened,
            "security_case.updated" => Self::SecurityCaseUpdated,
            "security_case.verification_requested" => Self::SecurityCaseVerificationRequested,
            "security_case.verified_fixed" => Self::SecurityCaseVerifiedFixed,
            "behavior.scheduled" => Self::BehaviorScheduled,
            "behavior.started" => Self::BehaviorStarted,
            "behavior.completed" => Self::BehaviorCompleted,
            "behavior.failed" => Self::BehaviorFailed,
            "behavior.side_effect_requested" => Self::BehaviorSideEffectRequested,
            "behavior.side_effect_completed" => Self::BehaviorSideEffectCompleted,
            other => Self::Other(other.to_string()),
        }
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for EventType {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for EventType {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Ok(EventType::from_name(&name))
    }
}

impl EventType {
    /// The name of the event type, as it is written in the ledger.
    pub fn as_str(&self) -> &str {
        match self {
            Self::EvidenceReceived => "evidence.received",
            Self::EvidenceRejected => "evidence.rejected",
            Self::EvidenceAccepted => "evidence.accepted",
            Self::EvidenceSuperseded => "evidence.superseded",
            Self::EvidenceVerificationChanged => "evidence.verification_changed",
            Self::ProjectionApplied => "projection.applied",
            Self::ProjectionFailed => "projection.failed",
            Self::ProjectionCheckpointed => "projection.checkpointed",
            Self::ProjectionRebuildStarted => "projection.rebuild_started",
            Self::ProjectionRebuildCompleted => "projection.rebuild_completed",
            Self::TrustEvaluationRequested => "trust.evaluation_requested",
            Self::TrustDecisionCreated => "trust.decision_created",
            Self::TrustDecisionSuperseded => "trust.decision_superseded",
            Self::PolicyVersionActivated => "policy.version_activated",
            Self::SecurityCaseOpened => "security_case.opened",
            Self::SecurityCaseUpdated => "security_case.updated",
            Self::SecurityCaseVerificationRequested => "security_case.verification_requested",
            Self::SecurityCaseVerifiedFixed => "security_case.verified_fixed",
            Self::BehaviorScheduled => "behavior.scheduled",
            Self::BehaviorStarted => "behavior.started",
            Self::BehaviorCompleted => "behavior.completed",
            Self::BehaviorFailed => "behavior.failed",
            Self::BehaviorSideEffectRequested => "behavior.side_effect_requested",
            Self::BehaviorSideEffectCompleted => "behavior.side_effect_completed",
            Self::Other(value) => value,
        }
    }

    /// Whether the event changes the state of the graph.
    ///
    /// The events that do not change anything are the ones about the failure
    /// of something: they are history, and they must not move the graph.
    pub fn mutates_graph(&self) -> bool {
        !matches!(
            self,
            Self::EvidenceRejected
                | Self::ProjectionFailed
                | Self::ProjectionCheckpointed
                | Self::ProjectionRebuildStarted
                | Self::BehaviorFailed
                | Self::EvidenceReceived
        )
    }
}

/// The payload of an event about an accepted piece of evidence.
///
/// It is a convenience for the common case: the envelope carries an opaque
/// payload, and this is the shape that fits the evidence events. It holds
/// identifiers and references, never content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidencePayload {
    /// The evidence the event is about.
    pub evidence_id: EvidenceId,
    /// The account it belongs to.
    pub tenant_hrn: Hrn,
    /// The digest of its content.
    pub content_digest: String,
    /// The evidence that this one supersedes, when it supersedes one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<EvidenceId>,
    /// The relation that the evidence claims, when it claims one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation_claim: Option<crate::domain::graph::RelationClaim>,
}

/// The envelope of an event.
///
/// The fields are private and there is no way to change them: an event that
/// is in the ledger is the history, and the history does not get edited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventEnvelope {
    id: EventId,
    tenant_hrn: Hrn,
    stream_id: String,
    stream_position: u64,
    event_type: EventType,
    schema_version: SchemaVersion,
    occurred_at: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
    producer: Producer,
    correlation_id: Option<String>,
    causation_id: Option<EventId>,
    idempotency_key: Option<IdempotencyKey>,
    payload: Value,
}

impl EventEnvelope {
    /// The envelope of an event.
    ///
    /// The fields of an event are private and there is no way to change
    /// them: an event that is in the ledger is the history, and the history
    /// does not get edited. A correction is another event.
    ///
    /// ```compile_fail
    /// # use hodei_evidence::{EventEnvelope, EventId, EventType};
    /// # use kernel::Hrn;
    /// # fn example(event: EventEnvelope) {
    /// let mut event = event;
    /// event.payload = serde_json::json!({"decided": true});
    /// # }
    /// ```
    ///
    /// Starts the building of an envelope.
    pub fn builder(id: EventId, tenant_hrn: Hrn, event_type: EventType) -> EventEnvelopeBuilder {
        EventEnvelopeBuilder {
            id,
            tenant_hrn,
            stream_id: String::new(),
            stream_position: 0,
            event_type,
            schema_version: SchemaVersion::initial(),
            occurred_at: Utc::now(),
            recorded_at: Utc::now(),
            producer: Producer::new("unknown", "unknown"),
            correlation_id: None,
            causation_id: None,
            idempotency_key: None,
            payload: Value::Null,
        }
    }

    /// The identity of the event.
    pub fn id(&self) -> &EventId {
        &self.id
    }

    /// The account the event belongs to.
    pub fn tenant_hrn(&self) -> &Hrn {
        &self.tenant_hrn
    }

    /// The stream the event belongs to.
    pub fn stream_id(&self) -> &str {
        &self.stream_id
    }

    /// The position of the event in its stream.
    pub fn stream_position(&self) -> u64 {
        self.stream_position
    }

    /// The same event, at another position of its stream.
    ///
    /// It is what a writer does when it reads the stream again and finds
    /// that the position it was going to write at is already taken: the
    /// event is the same event, and the position is the one that is free.
    pub fn with_position(&self, position: u64) -> Self {
        let mut event = self.clone();
        event.stream_position = position;
        event
    }

    /// What the event says happened.
    pub fn event_type(&self) -> &EventType {
        &self.event_type
    }

    /// The version of the schema that describes the payload.
    pub fn schema_version(&self) -> SchemaVersion {
        self.schema_version
    }

    /// When it happened.
    pub fn occurred_at(&self) -> DateTime<Utc> {
        self.occurred_at
    }

    /// When it was written.
    pub fn recorded_at(&self) -> DateTime<Utc> {
        self.recorded_at
    }

    /// Who produced it.
    pub fn producer(&self) -> &Producer {
        &self.producer
    }

    /// The correlation of the request that caused it.
    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// The event that caused this one.
    pub fn causation_id(&self) -> Option<&EventId> {
        self.causation_id.as_ref()
    }

    /// The key that makes a repeated delivery the same delivery.
    pub fn idempotency_key(&self) -> Option<&IdempotencyKey> {
        self.idempotency_key.as_ref()
    }

    /// The payload.
    pub fn payload(&self) -> &Value {
        &self.payload
    }

    /// The payload, read as the shape of an evidence event.
    ///
    /// It fails when the payload is of another shape, which is the point: an
    /// event of one type does not carry the payload of another.
    pub fn payload_as<T: serde::de::DeserializeOwned>(&self) -> Result<T, EventError> {
        serde_json::from_value(self.payload.clone())
            .map_err(|error| EventError::PayloadShape(error.to_string()))
    }
}

/// The error of an event that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EventError {
    /// The payload is not of the shape the event type declares.
    #[error("the payload is not of the expected shape: {0}")]
    PayloadShape(String),
    /// The envelope is not coherent.
    #[error("the event envelope is not valid: {0}")]
    InvalidEnvelope(String),
}

/// The builder of an event envelope.
pub struct EventEnvelopeBuilder {
    id: EventId,
    tenant_hrn: Hrn,
    stream_id: String,
    stream_position: u64,
    event_type: EventType,
    schema_version: SchemaVersion,
    occurred_at: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
    producer: Producer,
    correlation_id: Option<String>,
    causation_id: Option<EventId>,
    idempotency_key: Option<IdempotencyKey>,
    payload: Value,
}

impl EventEnvelopeBuilder {
    /// The stream the event belongs to.
    pub fn stream(mut self, stream_id: impl Into<String>, position: u64) -> Self {
        self.stream_id = stream_id.into();
        self.stream_position = position;
        self
    }

    /// The version of the schema that describes the payload.
    pub fn schema_version(mut self, version: SchemaVersion) -> Self {
        self.schema_version = version;
        self
    }

    /// When it happened.
    pub fn occurred_at(mut self, moment: DateTime<Utc>) -> Self {
        self.occurred_at = moment;
        self
    }

    /// When it was written.
    pub fn recorded_at(mut self, moment: DateTime<Utc>) -> Self {
        self.recorded_at = moment;
        self
    }

    /// Who produced it.
    pub fn producer(mut self, producer: Producer) -> Self {
        self.producer = producer;
        self
    }

    /// The correlation of the request that caused it.
    pub fn correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    /// The event that caused this one.
    pub fn causation_id(mut self, causation_id: EventId) -> Self {
        self.causation_id = Some(causation_id);
        self
    }

    /// The key that makes a repeated delivery the same delivery.
    pub fn idempotency_key(mut self, key: IdempotencyKey) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    /// The payload.
    pub fn payload(mut self, payload: Value) -> Self {
        self.payload = payload;
        self
    }

    /// The payload, built from something serializable.
    pub fn typed_payload<T: Serialize>(mut self, payload: T) -> Result<Self, EventError> {
        self.payload = serde_json::to_value(payload)
            .map_err(|error| EventError::PayloadShape(error.to_string()))?;
        Ok(self)
    }

    /// The envelope.
    pub fn build(self) -> EventEnvelope {
        EventEnvelope {
            id: self.id,
            tenant_hrn: self.tenant_hrn,
            stream_id: self.stream_id,
            stream_position: self.stream_position,
            event_type: self.event_type,
            schema_version: self.schema_version,
            occurred_at: self.occurred_at,
            recorded_at: self.recorded_at,
            producer: self.producer,
            correlation_id: self.correlation_id,
            causation_id: self.causation_id,
            idempotency_key: self.idempotency_key,
            payload: self.payload,
        }
    }
}
