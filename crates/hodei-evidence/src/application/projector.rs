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
use crate::domain::graph::{GraphChangeSet, GraphNode, GraphRelation, GraphRelationWithdrawal};
use crate::ports::graph::GraphProjector;

/// The identity of a piece of evidence, as a node of the graph.
///
/// The evidence lives in the account of the tenant that produced it, with
/// its own kind of resource, so that a node of the graph is named the same
/// way wherever it appears.
pub fn evidence_hrn(tenant: &kernel::Hrn, id: &crate::domain::evidence::EvidenceId) -> kernel::Hrn {
    // The identity of an evidence carries the algorithm of its digest, and
    // an identity does not hold a colon.
    let safe = id.as_str().replace(':', "-");
    kernel::Hrn::from_string(&format!(
        "hrn:{}:{}:{}:{}:Evidence/{}",
        tenant.partition(),
        tenant.service(),
        tenant.region(),
        tenant.account_id(),
        safe
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
    fn project(
        &self,
        event: &EventEnvelope,
    ) -> Result<GraphChangeSet, crate::ports::graph::ProjectionError> {
        let mut changes = GraphChangeSet::empty().with_cause(event.id().as_str());

        // Only the events that change the graph are projected. A rejection,
        // a failed projection or a checkpoint are history, not changes.
        if !event.event_type().mutates_graph() {
            return Ok(changes);
        }
        // A superseded evidence WITHDRAWS its support: una revision dice
        // que el apoyo anterior se va, uno, y los demas se quedan. The
        // withdrawal needs to know WHICH support it withdraws: without
        // the claim and without the superseded evidence, the event says
        // nothing about the graph.
        if matches!(event.event_type(), EventType::EvidenceSuperseded) {
            let Ok(payload) = event.payload_as::<EvidencePayload>() else {
                return Ok(changes);
            };
            let (Some(superseded), Some(claim)) = (payload.supersedes.clone(), payload.relation_claim.clone())
            else {
                return Ok(changes);
            };
            return Ok(changes.with_withdrawal(GraphRelationWithdrawal::new(
                claim.relation,
                claim.from,
                claim.to,
                payload.tenant_hrn.clone(),
                superseded,
                event.occurred_at(),
            )));
        }

        if !matches!(event.event_type(), EventType::EvidenceAccepted) {
            return Ok(changes);
        }

        let Ok(payload) = event.payload_as::<EvidencePayload>() else {
            // A payload that is not of the shape the event type declares says
            // nothing about the graph. It is not an error of the projection:
            // it is an event that does not project.
            return Ok(changes);
        };

        // The piece of evidence itself is a node.
        changes = changes.with_node(GraphNode::new(
            evidence_hrn(&payload.tenant_hrn, &payload.evidence_id),
            payload.tenant_hrn.clone(),
            crate::domain::graph::NodeKind::Evidence,
            event.occurred_at(),
        ));

        // And the relation it claims, with the evidence that supports it.
        let Some(claim) = payload.relation_claim.clone() else {
            return Ok(changes);
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

        Ok(changes)
    }
}
