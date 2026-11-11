//! The acceptance of an incoming piece of evidence.
//!
//! The vertical slice of the plan: a repository, a commit, a build, an
//! artifact and a deployment, arriving as evidence from a producer, and
//! accepted or refused with a reason.
//!
//! # Guarantees
//!
//! - I1 The whole journey is accepted with fixtures locales, and the
//!   evidence of each step says what it is about.
//! - I2 An invalid delivery is refused with a reason, and nothing is
//!   accepted: the store of evidence does not change.
//! - I3 The content is preserved by its digest and can be checked against
//!   the bytes that were delivered.
//! - I4 The same delivery twice is idempotent: the second one returns the
//!   record that already existed and writes nothing.
//! - I5 A revision is a new record: the previous one is kept.
//! - I6 An envelope that nobody vouched for is refused, and its content is
//!   not kept either.
//! - I7 The signature is over the canonical form: changing any field of the
//!   envelope invalidates it.

use chrono::{DateTime, TimeZone, Utc};
use hodei_evidence::{
    ContentDigest, ContentStore, EvidenceIngestion, EvidenceStore, EvidenceType,
    EvidenceVerification, IngestionEnvelope, IngestionOutcome, IngestionRequest, Normalization,
    Producer, RejectionReason, SourceRef,
};
use hodei_evidence_storage::{
    HmacEnvelopeAuthenticator, InMemoryContentStore, ProducerCredentials, SurrealEventLedger,
    SurrealEvidenceStore,
};
use kernel::Hrn;
use std::sync::Arc;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, RocksDb};

fn account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn subject(kind: &str, id: &str) -> Hrn {
    Hrn::from_string(&format!("hrn:hodei:artifacts:eu-west-1:acme:{kind}/{id}")).expect("an HRN")
}

fn moment(second: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800 + second, 0)
        .single()
        .expect("a moment")
}

fn producer() -> Producer {
    Producer::new("connector", "pipelinek").with_version("1.2.3")
}

/// The envelope of one step of the journey.
fn an_envelope(external_id: &str, content: &[u8], kind: EvidenceType) -> IngestionEnvelope {
    IngestionEnvelope {
        producer: producer(),
        tenant_hrn: account(),
        subject_refs: vec![subject("Artifact", "web-1.0.0")],
        evidence_type: kind,
        schema_uri: None,
        schema_version: None,
        source: SourceRef::by_external_id(external_id),
        content_digest: ContentDigest::of_bytes(content),
        relation_claim: None,
        observed_at: moment(10),
        valid_from: None,
        verification: EvidenceVerification::verified("signature"),
        normalization: Normalization::new("pipelinek", "1.0.0"),
        correlation_id: Some("corr-1".to_string()),
        signature: String::new(),
    }
}

/// The ingestion, over the real store and the real ledger.
async fn ingestion() -> (
    EvidenceIngestion,
    Arc<HmacEnvelopeAuthenticator>,
    Arc<InMemoryContentStore>,
    Arc<SurrealEvidenceStore<Db>>,
    Arc<SurrealEventLedger<Db>>,
) {
    let tmp = tempfile::tempdir().expect("a throwaway directory");
    // The directory has to outlive the ingestion, so it is leaked on
    // purpose: the test binary is short-lived.
    let path = tmp.keep();
    let db = Arc::new(
        Surreal::new::<RocksDb>(path.join("db").to_string_lossy().to_string())
            .await
            .expect("the database opens"),
    );
    db.use_ns("hodei")
        .use_db("evidence")
        .await
        .expect("the namespace");

    let content_store = Arc::new(InMemoryContentStore::new());
    let evidence_store = Arc::new(SurrealEvidenceStore::new(db.clone()));
    let ledger = Arc::new(SurrealEventLedger::new(db.clone()));
    let authenticator = Arc::new(HmacEnvelopeAuthenticator::new());
    authenticator.register(
        "pipelinek",
        ProducerCredentials {
            secret: b"the-secret-of-the-producer".to_vec(),
            accounts: vec!["acme".to_string()],
        },
    );

    let ingestion = EvidenceIngestion::new(
        content_store.clone(),
        evidence_store.clone(),
        ledger.clone(),
        authenticator.clone(),
    );
    (
        ingestion,
        authenticator,
        content_store,
        evidence_store,
        ledger,
    )
}

/// Signs an envelope the way the producer would.
fn signed(authenticator: &HmacEnvelopeAuthenticator, envelope: &mut IngestionEnvelope) {
    envelope.signature = authenticator.sign(envelope).expect("the producer is known");
}

/// I1 The whole journey is accepted.
#[tokio::test]
async fn i1_the_whole_journey_is_accepted() {
    let (ingestion, authenticator, content_store, evidence_store, _ledger) = ingestion().await;

    let steps: [(&str, &[u8], EvidenceType); 5] = [
        ("repo-1", b"the repository", EvidenceType::SourceMetadata),
        ("commit-1", b"the commit", EvidenceType::SourceMetadata),
        ("build-1", b"the build", EvidenceType::BuildMetadata),
        (
            "artifact-1",
            b"the artifact",
            EvidenceType::ArtifactIntegrity,
        ),
        (
            "deployment-1",
            b"the deployment",
            EvidenceType::DeploymentObservation,
        ),
    ];

    for (external_id, content, kind) in &steps {
        let mut envelope = an_envelope(external_id, content, kind.clone());
        signed(&authenticator, &mut envelope);

        let outcome = ingestion
            .ingest(IngestionRequest {
                envelope,
                content: content.to_vec(),
            })
            .await
            .expect("the ingestion runs");

        assert!(
            matches!(outcome, IngestionOutcome::Accepted { .. }),
            "the step {external_id} is accepted: {outcome:?}"
        );
    }

    // The five pieces of evidence are in the store.
    for (external_id, content, _) in &steps {
        let found = evidence_store
            .find_by_external_id(&producer(), external_id)
            .await
            .expect("the store is read");
        let found = found.expect("the evidence is there");
        assert_eq!(
            found.content().digest,
            ContentDigest::of_bytes(content),
            "the evidence of {external_id} is about the content that was delivered"
        );
    }

    // And the content of every step is preserved.
    assert_eq!(content_store.len(), 5, "the content is kept by digest");
}

/// I2 An invalid delivery is refused, and nothing is accepted.
#[tokio::test]
async fn i2_an_invalid_delivery_is_refused_and_nothing_is_accepted() {
    let (ingestion, authenticator, _content_store, evidence_store, _ledger) = ingestion().await;

    let content = b"the content that is not what was declared";
    let mut envelope = an_envelope("run-1", content, EvidenceType::Sbom);
    // The envelope declares the digest of something else.
    envelope.content_digest = ContentDigest::sha256("b".repeat(64));
    signed(&authenticator, &mut envelope);

    let outcome = ingestion
        .ingest(IngestionRequest {
            envelope,
            content: content.to_vec(),
        })
        .await
        .expect("the ingestion runs");

    assert!(
        matches!(
            outcome,
            IngestionOutcome::Rejected {
                reason: RejectionReason::ContentDigestMismatch
            }
        ),
        "the delivery that does not match its digest is refused: {outcome:?}"
    );

    let found = evidence_store
        .find_by_external_id(&producer(), "run-1")
        .await
        .expect("the store is read");
    assert!(found.is_none(), "nothing was accepted");
}

/// I3 The content is preserved by its digest and can be checked.
#[tokio::test]
async fn i3_the_content_is_preserved_and_can_be_checked() {
    let (ingestion, authenticator, content_store, _evidence_store, _ledger) = ingestion().await;

    let content = b"the raw bytes of the evidence";
    let mut envelope = an_envelope("run-1", content, EvidenceType::Sbom);
    signed(&authenticator, &mut envelope);

    let outcome = ingestion
        .ingest(IngestionRequest {
            envelope,
            content: content.to_vec(),
        })
        .await
        .expect("the ingestion runs");
    assert!(
        outcome.is_accepted(),
        "the delivery is accepted: {outcome:?}"
    );

    let digest = ContentDigest::of_bytes(content);
    let stored = content_store
        .get(&digest)
        .await
        .expect("the content is read");
    assert_eq!(
        stored.as_deref(),
        Some(content.as_slice()),
        "the content can be read back and checked against the digest"
    );
}

/// I4 The same delivery twice is idempotent.
#[tokio::test]
async fn i4_the_same_delivery_twice_is_idempotent() {
    let (ingestion, authenticator, content_store, _evidence_store, _ledger) = ingestion().await;

    let content = b"the same content";
    let mut envelope = an_envelope("run-1", content, EvidenceType::Sbom);
    signed(&authenticator, &mut envelope);

    let first = ingestion
        .ingest(IngestionRequest {
            envelope: envelope.clone(),
            content: content.to_vec(),
        })
        .await
        .expect("the first delivery is accepted");
    let second = ingestion
        .ingest(IngestionRequest {
            envelope: envelope.clone(),
            content: content.to_vec(),
        })
        .await
        .expect("the second delivery is accepted");

    assert!(
        matches!(first, IngestionOutcome::Accepted { .. }),
        "the first delivery is accepted: {first:?}"
    );
    assert!(
        matches!(second, IngestionOutcome::AlreadyAccepted { .. }),
        "the second one says that it was already accepted: {second:?}"
    );
    assert_eq!(
        first.evidence_id(),
        second.evidence_id(),
        "both deliveries name the same record"
    );
    assert_eq!(
        content_store.len(),
        1,
        "the content is kept once, because it is the same content"
    );
}

/// I5 A revision is a new record, and the previous one is kept.
#[tokio::test]
async fn i5_a_revision_keeps_the_previous_record() {
    let (ingestion, authenticator, _content_store, evidence_store, _ledger) = ingestion().await;

    let first_content = b"the first reading of the evidence";
    let mut envelope = an_envelope("run-1", first_content, EvidenceType::Sbom);
    signed(&authenticator, &mut envelope);
    let first = ingestion
        .ingest(IngestionRequest {
            envelope,
            content: first_content.to_vec(),
        })
        .await
        .expect("the first delivery is accepted");

    // The same external identity, different content: a revision.
    let second_content = b"the second reading of the evidence";
    let mut envelope = an_envelope("run-1", second_content, EvidenceType::Sbom);
    signed(&authenticator, &mut envelope);
    let second = ingestion
        .ingest(IngestionRequest {
            envelope,
            content: second_content.to_vec(),
        })
        .await
        .expect("the revision is accepted");

    let supersedes = match &second {
        IngestionOutcome::Revised { supersedes, .. } => supersedes.clone(),
        other => panic!("the second delivery is a revision: {other:?}"),
    };
    assert_eq!(
        supersedes,
        first
            .evidence_id()
            .expect("the first has an identity")
            .clone(),
        "the revision says what it revises"
    );

    // The previous record is still there: the history is not overwritten.
    let previous = evidence_store
        .get(&supersedes)
        .await
        .expect("the store is read");
    let previous = previous.expect("the previous record is kept");
    assert_eq!(
        previous.content().digest,
        ContentDigest::of_bytes(first_content),
        "the previous record still says what it said"
    );
}

/// I6 An envelope that nobody vouched for is refused.
#[tokio::test]
async fn i6_an_envelope_that_nobody_vouched_for_is_refused() {
    let (ingestion, _authenticator, content_store, evidence_store, _ledger) = ingestion().await;

    let content = b"the content of an envelope with a wrong signature";
    let mut envelope = an_envelope("run-1", content, EvidenceType::Sbom);
    envelope.signature = "not the signature".to_string();

    let outcome = ingestion
        .ingest(IngestionRequest {
            envelope,
            content: content.to_vec(),
        })
        .await
        .expect("the ingestion runs");

    assert!(
        matches!(
            outcome,
            IngestionOutcome::Rejected {
                reason: RejectionReason::Unauthenticated
            }
        ),
        "the envelope with a wrong signature is refused: {outcome:?}"
    );
    assert!(
        content_store.is_empty(),
        "the content of a refused envelope is not kept"
    );
    let found = evidence_store
        .find_by_external_id(&producer(), "run-1")
        .await
        .expect("the store is read");
    assert!(found.is_none(), "nothing was accepted");
}

/// I7 The signature is over the canonical form.
#[tokio::test]
async fn i7_the_signature_is_over_the_canonical_form() {
    let (_ingestion, authenticator, _content_store, _evidence_store, _ledger) = ingestion().await;

    let content = b"the content";
    let mut envelope = an_envelope("run-1", content, EvidenceType::Sbom);
    signed(&authenticator, &mut envelope);

    // The same envelope, signed: the same bytes, so the same signature.
    assert_eq!(
        authenticator.sign(&envelope),
        authenticator.sign(&envelope),
        "the canonical form is deterministic"
    );

    // Change one field: the signature no longer holds.
    let mut changed = envelope.clone();
    changed.subject_refs = vec![subject("Artifact", "other")];
    assert_ne!(
        authenticator.sign(&changed),
        authenticator.sign(&envelope),
        "changing the envelope changes the canonical form"
    );
}
