//! The acceptance of an incoming piece of evidence.
//!
//! This is the pipeline of the plan, from the envelope that arrives to the
//! event that records what was accepted:
//!
//! ```text
//! receive -> authenticate producer -> validate envelope/schema ->
//! calculate content digest -> deduplicate/idempotency check ->
//! preserve raw evidence -> normalize deterministically ->
//! validate canonical identities -> accept/reject -> append accepted event
//! ```
//!
//! Two rules run through all of it:
//!
//! - **Nothing that failed acceptance mutates anything.** A rejection writes
//!   its event --a refusal is history-- and leaves the store of evidence and
//!   the graph untouched.
//! - **The history is not overwritten.** The same delivery twice returns
//!   the result that already exists; the same external identity with
//!   different content is a new revision that keeps the previous one.

use crate::domain::content::ContentDigest;
use crate::domain::event::{EventEnvelope, EventId, EventType, EvidencePayload};
use crate::domain::evidence::{EvidenceId, EvidenceRecord};
use crate::domain::idempotency::IdempotencyKey;
use crate::domain::ingestion::{IngestionEnvelope, IngestionOutcome, RejectionReason};
#[allow(unused_imports)]
use crate::ports::EventLedger as _;
use crate::ports::{
    ContentStore, EnvelopeAuthenticator, EventLedger, EvidenceStore, ExpectedVersion,
};
use chrono::Utc;
use std::sync::Arc;

/// The request of an ingestion: the envelope and the bytes it refers to.
#[derive(Debug, Clone)]
pub struct IngestionRequest {
    /// The envelope the producer delivered.
    pub envelope: IngestionEnvelope,
    /// The content the envelope refers to.
    pub content: Vec<u8>,
}

/// The acceptance of an incoming piece of evidence.
pub struct EvidenceIngestion {
    content_store: Arc<dyn ContentStore>,
    evidence_store: Arc<dyn EvidenceStore>,
    ledger: Arc<dyn EventLedger>,
    authenticator: Arc<dyn EnvelopeAuthenticator>,
}

impl EvidenceIngestion {
    /// The acceptance, over the four ports it needs.
    pub fn new(
        content_store: Arc<dyn ContentStore>,
        evidence_store: Arc<dyn EvidenceStore>,
        ledger: Arc<dyn EventLedger>,
        authenticator: Arc<dyn EnvelopeAuthenticator>,
    ) -> Self {
        Self {
            content_store,
            evidence_store,
            ledger,
            authenticator,
        }
    }

    /// Accepts an incoming piece of evidence.
    ///
    /// The outcome says what happened: it was accepted, it had been accepted
    /// already, it is a revision of something that was accepted, or it was
    /// refused with a reason.
    pub async fn ingest(
        &self,
        request: IngestionRequest,
    ) -> Result<IngestionOutcome, IngestionError> {
        let IngestionRequest { envelope, content } = request;

        // 1. The producer is vouched for BEFORE anything else happens.
        if let Err(error) = self.authenticator.authenticate(&envelope).await {
            return self.reject(&envelope, rejection_for(error)).await;
        }

        // 2. The envelope has to be coherent.
        if let Err(reason) = validate_envelope(&envelope) {
            return self.reject(&envelope, reason).await;
        }

        // 3. The content has to be the content that was declared.
        let actual = ContentDigest::of_bytes(&content);
        if actual != envelope.content_digest {
            return self
                .reject(&envelope, RejectionReason::ContentDigestMismatch)
                .await;
        }

        // 4. The key of the delivery: the producer, the external identity,
        //    the content and the reading of the content.
        let key = IdempotencyKey::derive(
            &envelope.producer,
            &envelope.source,
            &envelope.content_digest,
            envelope.normalization.major_version(),
        );

        // 5. The content is preserved before the record exists: a claim
        //    without the bytes it claims cannot be checked later.
        self.content_store
            .put(content)
            .await
            .map_err(|error| IngestionError::Store(error.to_string()))?;

        // 6. The external identity of the producer says whether this is the
        //    first delivery of this thing, a repetition of it, or a new
        //    revision of it.
        let previous = match envelope.source.external_id.as_deref() {
            Some(external_id) => self
                .evidence_store
                .find_by_external_id(&envelope.producer, external_id)
                .await
                .map_err(|error| IngestionError::Store(error.to_string()))?,
            None => None,
        };

        // A delivery with the same content is the same delivery: nothing is
        // written and the record that exists is returned.
        if let Some(previous) = &previous
            && previous.content().digest == envelope.content_digest
        {
            return Ok(IngestionOutcome::AlreadyAccepted {
                evidence_id: previous.id().clone(),
            });
        }

        // 7. The record of the evidence.
        let record = record_from(&envelope);
        let evidence_id = record.id().clone();
        self.evidence_store
            .put(record)
            .await
            .map_err(|error| IngestionError::Store(error.to_string()))?;

        // 8. The event that records the acceptance, and the revision when
        //    this delivery revises something that was accepted before. The
        //    previous record is kept: the history is not overwritten.
        let supersedes = previous.as_ref().map(|previous| previous.id().clone());
        let event_id = self
            .append_event(
                &envelope,
                &key,
                EventType::EvidenceAccepted,
                supersedes.clone(),
            )
            .await?;

        Ok(match supersedes {
            Some(supersedes) => IngestionOutcome::Revised {
                evidence_id,
                supersedes,
                event_id,
            },
            None => IngestionOutcome::Accepted {
                evidence_id,
                event_id,
            },
        })
    }

    /// Records a refusal.
    ///
    /// A refusal is history: it is written down, so that the next producer
    /// that sends the same thing knows it was refused before. It does not
    /// touch the store of evidence, and it does not move the graph.
    async fn reject(
        &self,
        envelope: &IngestionEnvelope,
        reason: RejectionReason,
    ) -> Result<IngestionOutcome, IngestionError> {
        let key = IdempotencyKey::derive(
            &envelope.producer,
            &envelope.source,
            &envelope.content_digest,
            envelope.normalization.major_version(),
        );
        let _ = self
            .append_event(envelope, &key, EventType::EvidenceRejected, None)
            .await;
        tracing::info!(
            producer = %envelope.producer.id,
            reason = %reason,
            "evidence rejected"
        );
        Ok(IngestionOutcome::Rejected { reason })
    }

    /// Appends the event that records what happened.
    async fn append_event(
        &self,
        envelope: &IngestionEnvelope,
        key: &IdempotencyKey,
        event_type: EventType,
        supersedes: Option<EvidenceId>,
    ) -> Result<EventId, IngestionError> {
        // The writer asks where the stream is and writes at the position
        // that follows. That is the optimistic check: if another writer got
        // there first, the ledger says so.
        let stream = stream_of(&envelope.tenant_hrn);
        let current = self
            .ledger
            .position(&stream)
            .await
            .map_err(|error| IngestionError::Ledger(error.to_string()))?;
        let expected = match current {
            Some(position) => ExpectedVersion::At(position),
            None => ExpectedVersion::Absent,
        };
        let position = expected.next_position();

        let payload = EvidencePayload {
            evidence_id: EvidenceId::new(envelope.content_digest.to_string()),
            tenant_hrn: envelope.tenant_hrn.clone(),
            content_digest: envelope.content_digest.to_string(),
            supersedes,
        };

        let event = EventEnvelope::builder(
            EventId::new(format!("evt-{}", envelope.content_digest)),
            envelope.tenant_hrn.clone(),
            event_type,
        )
        .stream(stream, position)
        .occurred_at(envelope.observed_at)
        .recorded_at(Utc::now())
        .producer(envelope.producer.clone())
        .idempotency_key(key.clone())
        .typed_payload(payload)
        .map_err(|error| IngestionError::Contract(error.to_string()))?
        .build();

        let id = self
            .ledger
            .append(expected, event)
            .await
            .map_err(|error| IngestionError::Ledger(error.to_string()))?;
        Ok(id)
    }
}

/// The error of an ingestion that could not be carried out.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IngestionError {
    /// A port was not available.
    #[error("the store is not available: {0}")]
    Store(String),
    /// The ledger refused the event.
    #[error("the ledger refused the event: {0}")]
    Ledger(String),
    /// A contract could not be built.
    #[error("a contract could not be built: {0}")]
    Contract(String),
}

/// The reason of a refusal, given the error of the authenticator.
fn rejection_for(error: crate::ports::AuthenticationError) -> RejectionReason {
    use crate::ports::AuthenticationError;
    match error {
        AuthenticationError::InvalidSignature
        | AuthenticationError::UnknownProducer
        | AuthenticationError::NotAllowedForAccount => RejectionReason::Unauthenticated,
        AuthenticationError::Unavailable => RejectionReason::Unavailable,
    }
}

/// Checks that the envelope is coherent.
fn validate_envelope(envelope: &IngestionEnvelope) -> Result<(), RejectionReason> {
    if envelope.tenant_hrn.account_id().is_empty() {
        return Err(RejectionReason::InvalidEnvelope(
            "the envelope has no account".to_string(),
        ));
    }
    if envelope.producer.id.trim().is_empty() {
        return Err(RejectionReason::InvalidEnvelope(
            "the envelope has no producer".to_string(),
        ));
    }
    if !envelope.content_digest.is_well_formed() {
        return Err(RejectionReason::InvalidEnvelope(
            "the declared digest is not well formed".to_string(),
        ));
    }
    Ok(())
}

/// The record of the evidence of an envelope.
fn record_from(envelope: &IngestionEnvelope) -> EvidenceRecord {
    EvidenceRecord::builder(
        EvidenceId::new(format!("evd-{}", envelope.content_digest)),
        envelope.tenant_hrn.clone(),
    )
    .subjects(envelope.subject_refs.clone())
    .evidence_type(envelope.evidence_type.clone())
    .producer(envelope.producer.clone())
    .source(envelope.source.clone())
    .content(crate::domain::content::ContentRef::by_digest(
        envelope.content_digest.clone(),
    ))
    .observed_at(envelope.observed_at)
    .verification(envelope.verification.clone())
    .normalization(envelope.normalization.clone())
    .correlation_id(envelope.correlation_id.clone().unwrap_or_default())
    .build()
}

/// The stream of the events of an account.
fn stream_of(tenant: &kernel::Hrn) -> String {
    format!("tenant-{}", tenant.account_id())
}
