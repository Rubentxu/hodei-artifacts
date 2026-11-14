//! The projection of the events into the graph.
//!
//! The projector is a pure function of the events, and that is the property
//! that makes the graph rebuildable: the rebuild replays the ledger through
//! the same projector and gets the same graph.
//!
//! # Guarantees
//!
//! - P1 The same event always projects the same changes.
//! - P2 An accepted event with a claim projects the two nodes and the
//!   relation, and the relation carries the evidence that supports it.
//! - P3 An event that changes nothing projects nothing: a rejection is
//!   history, not a change.
//! - P4 A relation is identified by its type and its two ends, so the same
//!   relation with more support is one relation.

use chrono::{DateTime, TimeZone, Utc};
use hodei_evidence::{
    EventEnvelope, EventId, EventType, EvidenceGraphProjector, EvidenceId, EvidencePayload,
    GraphProjector, NodeKind, RelationClaim, RelationType,
};
use kernel::Hrn;

fn account() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn build() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Build/web-1.0.0-42").expect("an HRN")
}

fn artifact() -> Hrn {
    Hrn::from_string("hrn:hodei:artifacts:eu-west-1:acme:Artifact/web-1.0.0").expect("an HRN")
}

fn moment() -> DateTime<Utc> {
    Utc.timestamp_opt(1_767_322_800, 0)
        .single()
        .expect("a moment")
}

/// The event of an accepted piece of evidence that claims a relation.
fn accepted_event() -> EventEnvelope {
    let claim = RelationClaim {
        relation: RelationType::Produced,
        from: build(),
        to: artifact(),
        from_kind: NodeKind::Build,
        to_kind: NodeKind::Artifact,
    };
    let payload = EvidencePayload {
        evidence_id: EvidenceId::new("evd-1"),
        tenant_hrn: account(),
        content_digest: "sha256:aa".to_string(),
        supersedes: None,
        relation_claim: Some(claim),
    };

    EventEnvelope::builder(
        EventId::new("evt-1"),
        account(),
        EventType::EvidenceAccepted,
    )
    .stream("tenant-acme", 0)
    .occurred_at(moment())
    .recorded_at(moment())
    .typed_payload(payload)
    .expect("the payload serializes")
    .build()
}

/// P1 The same event always projects the same changes.
#[test]
fn p1_the_projection_is_deterministic() {
    let projector = EvidenceGraphProjector::new();
    let event = accepted_event();

    let first = projector.project(&event).expect("the event projects");
    let second = projector.project(&event).expect("the event projects");

    assert_eq!(first, second, "the same event projects the same changes");
    assert!(!first.is_empty(), "the event projects something");
}

/// P2 An accepted event with a claim projects the nodes and the relation.
#[test]
fn p2_an_accepted_event_projects_the_nodes_and_the_relation() {
    let projector = EvidenceGraphProjector::new();
    let changes = projector
        .project(&accepted_event())
        .expect("the event projects");

    // The two nodes of the relation, plus the evidence itself.
    let kinds: Vec<&str> = changes
        .nodes
        .iter()
        .map(|node| node.kind.as_str())
        .collect();
    assert!(
        kinds.contains(&"build") && kinds.contains(&"artifact") && kinds.contains(&"evidence"),
        "the evidence and the two ends of the relation are nodes: {kinds:?}"
    );

    // And the relation, with the evidence that supports it.
    assert_eq!(changes.relations.len(), 1, "one relation is projected");
    let relation = &changes.relations[0];
    assert_eq!(relation.relation, RelationType::Produced);
    assert_eq!(relation.from, build());
    assert_eq!(relation.to, artifact());
    assert_eq!(
        relation.evidence_id,
        EvidenceId::new("evd-1"),
        "the relation says which evidence supports it"
    );
    assert_eq!(
        relation.tenant_hrn,
        account(),
        "the relation belongs to an account"
    );
}

/// P3 An event that changes nothing projects nothing.
#[test]
fn p3_an_event_that_changes_nothing_projects_nothing() {
    let projector = EvidenceGraphProjector::new();

    let rejected = EventEnvelope::builder(
        EventId::new("evt-2"),
        account(),
        EventType::EvidenceRejected,
    )
    .stream("tenant-acme", 1)
    .occurred_at(moment())
    .recorded_at(moment())
    .payload(serde_json::json!({"rejected": true}))
    .build();

    assert!(
        projector
            .project(&rejected)
            .expect("the event projects")
            .is_empty(),
        "a rejection is history, not a change"
    );
}

/// P4 A relation is identified by its type and its two ends.
#[test]
fn p4_a_relation_is_identified_by_its_type_and_its_ends() {
    let projector = EvidenceGraphProjector::new();
    let changes = projector
        .project(&accepted_event())
        .expect("the event projects");
    let relation = &changes.relations[0];

    assert_eq!(
        relation.identity(),
        format!("produced|{}|{}", build(), artifact()),
        "the identity of a relation is its type and its two ends"
    );
}

/// The event of a superseded piece of evidence: the NEW evidence says
/// which one it supersedes, and claims the SAME relation with its own
/// facts.
fn superseded_event() -> EventEnvelope {
    let claim = RelationClaim {
        relation: RelationType::Produced,
        from: build(),
        to: artifact(),
        from_kind: NodeKind::Build,
        to_kind: NodeKind::Artifact,
    };
    let payload = EvidencePayload {
        evidence_id: EvidenceId::new("evd-2"),
        tenant_hrn: account(),
        content_digest: "sha256:bb".to_string(),
        supersedes: Some(EvidenceId::new("evd-1")),
        relation_claim: Some(claim),
    };

    EventEnvelope::builder(
        EventId::new("evt-2"),
        account(),
        EventType::EvidenceSuperseded,
    )
    .stream("tenant-acme", 1)
    .occurred_at(moment())
    .recorded_at(moment())
    .typed_payload(payload)
    .expect("the payload serializes")
    .build()
}

/// P5 A superseded event projects the WITHDRAWAL of the support the
/// superseded evidence had on the claimed relation, and it projects
/// NOTHING ELSE: the new evidence's own support was already said by its
/// acceptance.
#[test]
fn p5_a_superseded_event_withdraws_the_support_of_the_superseded_evidence() {
    let projector = EvidenceGraphProjector::new();
    let changes = projector
        .project(&superseded_event())
        .expect("the event projects");

    assert_eq!(
        changes.relations.len(),
        0,
        "the superseded event does not re-assert anything: {:?}",
        changes.relations
    );
    assert_eq!(
        changes.withdrawals.len(),
        1,
        "the superseded event withdraws one support: {:?}",
        changes.withdrawals
    );
    let withdrawal = &changes.withdrawals[0];
    assert_eq!(
        withdrawal.evidence_id,
        EvidenceId::new("evd-1"),
        "the support withdrawn is the SUPERSEDED evidence's, not the new one's"
    );
    assert_eq!(withdrawal.relation, RelationType::Produced);
    assert_eq!(withdrawal.from, build());
    assert_eq!(withdrawal.to, artifact());
    assert_eq!(withdrawal.tenant_hrn, account());
}

/// P6 A superseded event without a claim or without the evidence it
/// supersedes says nothing about the graph: no withdrawal can be said
/// without knowing WHICH support it withdraws.
#[test]
fn p6_a_superseded_event_without_a_claim_projects_nothing() {
    let projector = EvidenceGraphProjector::new();

    // Sin claim: no se sabe que apoyo se retira.
    let no_claim = EvidencePayload {
        evidence_id: EvidenceId::new("evd-2"),
        tenant_hrn: account(),
        content_digest: "sha256:bb".to_string(),
        supersedes: Some(EvidenceId::new("evd-1")),
        relation_claim: None,
    };
    let event = EventEnvelope::builder(
        EventId::new("evt-3"),
        account(),
        EventType::EvidenceSuperseded,
    )
    .typed_payload(no_claim)
    .expect("the payload serializes")
    .build();
    let changes = projector.project(&event).expect("the event projects");
    assert!(
        changes.is_empty(),
        "without a claim there is no withdrawal: {changes:?}"
    );

    // Sin supersedes: no hay apoyo anterior que retirar.
    let no_supersedes = EvidencePayload {
        evidence_id: EvidenceId::new("evd-2"),
        tenant_hrn: account(),
        content_digest: "sha256:bb".to_string(),
        supersedes: None,
        relation_claim: Some(RelationClaim {
            relation: RelationType::Produced,
            from: build(),
            to: artifact(),
            from_kind: NodeKind::Build,
            to_kind: NodeKind::Artifact,
        }),
    };
    let event = EventEnvelope::builder(
        EventId::new("evt-4"),
        account(),
        EventType::EvidenceSuperseded,
    )
    .typed_payload(no_supersedes)
    .expect("the payload serializes")
    .build();
    let changes = projector.project(&event).expect("the event projects");
    assert!(
        changes.is_empty(),
        "without a superseded evidence there is no withdrawal: {changes:?}"
    );
}
