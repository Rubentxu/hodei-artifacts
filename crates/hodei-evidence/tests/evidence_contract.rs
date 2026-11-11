//! The public contract of the evidence context.
//!
//! These are the guarantees that the rest of the product relies on. They are
//! about the *shape* of the contracts, not about any adapter: the evidence
//! context has no adapter yet, and that is deliberate.
//!
//! # Guarantees
//!
//! - C1 A record references its content; it does not carry it. A record has
//!   no field where a raw payload, and therefore a secret, would go.
//! - C2 The temporal facts are kept apart: when Hodei learned it, when the
//!   source says it became true, and when it was accepted.
//! - C3 Only verified evidence is a proof. Unverified is a claim and invalid
//!   is a contradiction; neither counts as a verification.
//! - C4 The idempotency key tells a repeated delivery from a new revision,
//!   and a revision does not overwrite the history.
//! - C5 An event that does not change anything does not claim to change the
//!   graph.
//! - C6 A reader of a schema version knows which versions it can read, and
//!   an upcaster is explicit.
//! - C7 The contracts do not mention a database, an HTTP framework or a
//!   transaction.

use chrono::{DateTime, TimeZone, Utc};
use hodei_evidence::{
    ContentDigest, ContentRef, EventEnvelope, EventId, EventType, EvidenceId, EvidenceRecord,
    EvidenceType, EvidenceVerification, IdempotencyKey, Normalization, Producer, Redelivery,
    SchemaVersion, SourceRef,
};
use kernel::Hrn;

fn account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/releases-1").expect("an HRN")
}

fn artifact() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn moment(second: u32) -> DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800 + i64::from(second), 0)
        .single()
        .expect("a moment")
}

fn digest(seed: &str) -> ContentDigest {
    let mut value = String::new();
    while value.len() < 64 {
        value.push_str(seed);
    }
    ContentDigest::sha256(&value[..64])
}

fn producer() -> Producer {
    Producer::new("connector", "pipelinek").with_version("1.2.3")
}

fn record(id: &str) -> EvidenceRecord {
    EvidenceRecord::builder(EvidenceId::new(id), account())
        .subjects(vec![artifact()])
        .evidence_type(EvidenceType::Sbom)
        .schema("https://cyclonedx.org/schema/bom-1.7.schema.json", "1.7")
        .producer(producer())
        .source(SourceRef::by_external_id("run-481"))
        .content(ContentRef::new(digest("a"), "content/sbom-481"))
        .observed_at(moment(10))
        .valid_from(moment(5))
        .recorded_at(moment(20))
        .verification(EvidenceVerification::verified("signature"))
        .normalization(Normalization::new("cyclonedx", "1.2.0"))
        .correlation_id("corr-1")
        .build()
}

/// C1 A record references its content; it does not carry it.
#[test]
fn c1_a_record_references_its_content() {
    let record = record("evd-1");
    assert_eq!(record.content().digest, digest("a"));
    assert_eq!(
        record.content().object_ref.as_deref(),
        Some("content/sbom-481")
    );

    // The whole record, written out, has nowhere to put a payload: it holds
    // the digest and where the content lives, and nothing else.
    let written = serde_json::to_string(&record).expect("the record serializes");
    assert!(
        written.contains("sha256") && written.contains(&digest("a").value),
        "the digest is in the record: {written}"
    );
    assert!(
        !written.contains("\"payload\"") && !written.contains("\"raw\""),
        "the record must not carry a payload: {written}"
    );
}

/// C2 The temporal facts are kept apart.
#[test]
fn c2_the_temporal_facts_are_kept_apart() {
    let record = record("evd-1");
    assert_eq!(record.observed_at(), moment(10));
    assert_eq!(record.valid_from(), Some(moment(5)));
    assert_eq!(record.recorded_at(), moment(20));
    assert_eq!(record.valid_to(), None);
}

/// C3 Only verified evidence is a proof.
#[test]
fn c3_only_verified_evidence_is_a_proof() {
    assert!(EvidenceVerification::verified("signature").proves());
    assert!(!EvidenceVerification::unverified().proves());
    assert!(!EvidenceVerification::invalid("digest").proves());
}

/// C4 The idempotency key tells a repeated delivery from a new revision.
#[test]
fn c4_the_idempotency_key_tells_a_repeat_from_a_revision() {
    let source = SourceRef::by_external_id("run-481");

    let first = IdempotencyKey::derive(&producer(), &source, &digest("a"), "1");
    let repeat = IdempotencyKey::derive(&producer(), &source, &digest("a"), "1");
    let revision = IdempotencyKey::derive(&producer(), &source, &digest("b"), "1");

    assert_eq!(
        first.classify_redelivery(&repeat, &producer(), &source, &digest("a")),
        Redelivery::SameDelivery
    );
    assert_eq!(
        first.classify_redelivery(&revision, &producer(), &source, &digest("b")),
        Redelivery::NewRevision,
        "the same external identity with other content is a revision, not an overwrite"
    );
}

/// C5 An event that does not change anything does not claim to change the
/// graph.
#[test]
fn c5_an_event_that_changes_nothing_does_not_claim_to_change_the_graph() {
    assert!(EventType::EvidenceAccepted.mutates_graph());
    assert!(EventType::ProjectionApplied.mutates_graph());
    assert!(!EventType::EvidenceRejected.mutates_graph());
    assert!(!EventType::ProjectionFailed.mutates_graph());
    assert!(!EventType::EvidenceReceived.mutates_graph());
}

/// C6 A reader knows which versions it can read, and an upcaster is explicit.
#[test]
fn c6_a_reader_knows_which_versions_it_can_read() {
    let reader = SchemaVersion::new(1, 4);
    assert!(reader.can_read(SchemaVersion::new(1, 0)));
    assert!(!reader.can_read(SchemaVersion::new(2, 0)));
}

/// The event carries the history it needs, and the payload is read as the
/// shape the event type declares.
#[test]
fn the_event_carries_the_history_and_its_payload() {
    let payload = hodei_evidence::EvidencePayload {
        evidence_id: EvidenceId::new("evd-1"),
        tenant_hrn: account(),
        content_digest: digest("a").to_string(),
        supersedes: None,
        relation_claim: None,
    };

    let event = EventEnvelope::builder(
        EventId::new("evt-1"),
        account(),
        EventType::EvidenceAccepted,
    )
    .stream("stream-1", 481)
    .schema_version(SchemaVersion::new(1, 0))
    .occurred_at(moment(10))
    .recorded_at(moment(20))
    .producer(producer())
    .correlation_id("corr-1")
    .idempotency_key(IdempotencyKey::derive(
        &producer(),
        &SourceRef::by_external_id("run-481"),
        &digest("a"),
        "1",
    ))
    .typed_payload(payload.clone())
    .expect("the payload serializes")
    .build();

    assert_eq!(event.stream_id(), "stream-1");
    assert_eq!(event.stream_position(), 481);
    assert_eq!(event.correlation_id(), Some("corr-1"));
    assert!(event.causation_id().is_none());
    assert!(event.idempotency_key().is_some());

    let read: hodei_evidence::EvidencePayload = event.payload_as().expect("the payload is read");
    assert_eq!(read.evidence_id, payload.evidence_id);
    assert_eq!(read.content_digest, payload.content_digest);
}
/// C8 The type of an event survives the round trip through the history.
///
/// The name of a type is what the ledger writes down and what it reads
/// back. A type that is written one way and read another is a type that
/// stops being recognized the moment it comes back from the store, and
/// everything that decides what an event means stops working with it.
#[test]
fn c8_the_event_type_survives_the_round_trip() {
    let types = [
        EventType::EvidenceReceived,
        EventType::EvidenceRejected,
        EventType::EvidenceAccepted,
        EventType::ProjectionApplied,
        EventType::ProjectionRebuildCompleted,
        EventType::TrustDecisionCreated,
        EventType::BehaviorSideEffectCompleted,
    ];

    for event_type in types {
        let json = serde_json::to_string(&event_type).expect("the type serializes");
        let read: EventType = serde_json::from_str(&json).expect("the type is read");
        assert_eq!(
            read, event_type,
            "the type {json} comes back as the same type"
        );
        assert_eq!(
            event_type.as_str(),
            json.trim_matches('"'),
            "the name of a type and the way it is written are the same"
        );
    }
}

/// C9 A type that this version does not know is kept, not refused.
///
/// The history has to be able to hold an event of a type that came later:
/// a version that cannot read it has to say so, not pretend it never
/// happened.
#[test]
fn c9_a_type_that_is_not_known_is_kept() {
    let json = "\"evidence.something_new\"";
    let read: EventType = serde_json::from_str(json).expect("the type is read");
    assert_eq!(read, EventType::Other("evidence.something_new".to_string()));
    assert_eq!(read.as_str(), "evidence.something_new");
    assert!(
        read.mutates_graph(),
        "an event that changes something still changes it, even when the type is not known"
    );
}
