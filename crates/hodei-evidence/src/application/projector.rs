//! The projector of the graph.
//!
//! The projector reads the events that were accepted and says what the graph
//! has to look like. It is a **pure function of the events**: the same events
//! produce the same change set, whatever order they are read in and however
//! many times they are read. That is what makes the graph rebuildable: the
//! rebuild replays the ledger through the same projector and gets the same
//! graph.
//!
//! It only projects the events that change the graph. A rejection is history
//! and not a change, and the projector says so by projecting nothing.

use crate::domain::event::{EventEnvelope, EventType, EvidencePayload};
use crate::domain::graph::{GraphChangeSet, GraphNode, GraphRelation};
use crate::ports::graph::GraphProjector;

/// The identity of a piece of evidence, as a node of the graph.
///
/// The evidence is not an artefact and does not live in the account's
/// namespace of resources, so it has its own identity: the one it was
/// given when it was accepted.
fn evidence_hrn(id: &crate::domain::evidence::EvidenceId) -> kernel::Hrn {
    kernel::Hrn::from_string(&format!(
        "hrn:hodei:evidence::global:Evidence/{}",
        id.as_str()
    ))
    .expect("the identity of an evidence is a valid HRN")
}

/// The projector of the events of the evidence into the graph.
#[derive(Debug, Clone, Default)]
pub struct EvidenceGraphProjector;

impl EvidenceGraphProjector {
    /// A projector.
    pub fn new() -> Self {
        Self
    }
}

impl GraphProjector for EvidenceGraphProjector {
    /// The changes that an event implies.
    fn project(&self, event: &EventEnvelope) -> GraphChangeSet {
        let mut changes = GraphChangeSet::empty();

        // Only the events that change the graph are projected. A rejection,
        // a failed projection or a checkpoint are history, not changes.
        if !event.event_type().mutates_graph() {
            return changes;
        }
        if !matches!(event.event_type(), EventType::EvidenceAccepted) {
            return changes;
        }

        let Ok(payload) = event.payload_as::<EvidencePayload>() else {
            // A payload that is not of the shape the event type declares says
            // nothing about the graph. It is not an error of the projection:
            // it is an event that does not project.
            return changes;
        };

        // The piece of evidence itself is a node.
        changes = changes.with_node(GraphNode::new(
            evidence_hrn(&payload.evidence_id),
            payload.tenant_hrn.clone(),
            crate::domain::graph::NodeKind::Evidence,
            event.occurred_at(),
        ));

        // And the relation it claims, with the evidence that supports it.
        let Some(claim) = payload.relation_claim.clone() else {
            return changes;
        };

        changes = changes
            .with_node(GraphNode::new(
                claim.from.clone(),
                payload.tenant_hrn.clone(),
                claim.from_kind.clone(),
                event.occurred_at(),
            ))
            .with_node(GraphNode::new(
                claim.to.clone(),
                payload.tenant_hrn.clone(),
                claim.to_kind.clone(),
                event.occurred_at(),
            ))
            .with_relation(GraphRelation::new(
                claim.relation.clone(),
                claim.from.clone(),
                claim.to.clone(),
                payload.tenant_hrn.clone(),
                payload.evidence_id.clone(),
                event.occurred_at(),
            ));

        changes
    }
}
