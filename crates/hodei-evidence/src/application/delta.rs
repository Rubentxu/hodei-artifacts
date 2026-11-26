//! The emission of the deltas of the relations.
//!
//! The emitter is a **pure function of the changeset and the supports
//! that were there before it**: the same changeset over the same state
//! says the same deltas, whatever order it is read in and however many
//! times. It does not touch a store, and it does not decide what a
//! delta means: it says what the changeset CHANGED about the supports
//! of the relations it names.
//!
//! A change that does not alter the meaning of a relation says NOTHING:
//! re-saying a support that has already been said is not a change, and withdrawing a
//! support that was never there does not withdraw it. That is the work that is not
//! repeated.

use std::collections::BTreeMap;

use crate::domain::delta::{RelationDelta, RelationDeltaKind};
use crate::domain::evidence::EvidenceId;
use crate::domain::graph::GraphChangeSet;
use chrono::{DateTime, Utc};

/// The supports that were there BEFORE the changeset, by the identity of
/// their relation. The emitter compares against them: what was already
/// said is not said again.
pub type SupportSnapshot = BTreeMap<String, Vec<EvidenceId>>;

/// The deltas a changeset says, against the supports that were there
/// before it.
///
/// The cause is the identity of what brings the changeset -- the event
/// or the changeset -- and is the key of the downstream deduplication: the
/// same cause said twice is ONE delta.
pub fn relation_deltas(
    cause: &str,
    changes: &GraphChangeSet,
    before: &SupportSnapshot,
    observed_at: DateTime<Utc>,
) -> Vec<RelationDelta> {
    let mut deltas = Vec::new();

    // What the changeset ASSERTS: a relation that had no supports is
    // ADDED, and one that already held others gains one more. A support
    // that was already there says nothing.
    for relation in &changes.relations {
        let identity = relation.identity();
        let existing = before.get(&identity);
        let already = existing
            .map(|supports| supports.iter().any(|one| one == &relation.evidence_id))
            .unwrap_or(false);
        if already {
            continue;
        }
        let kind = match existing {
            Some(supports) if !supports.is_empty() => RelationDeltaKind::SupportChanged,
            _ => RelationDeltaKind::Added,
        };
        deltas.push(RelationDelta {
            kind,
            tenant: relation.tenant_hrn.clone(),
            relation: relation.relation.clone(),
            from: relation.from.clone(),
            to: relation.to.clone(),
            evidence: relation.evidence_id.clone(),
            cause: cause.to_string(),
            observed_at,
        });
    }

    // What the changeset WITHDRAWS: only a support that WAS there is
    // removed. Withdrawing what is not there changes nothing, and what does not
    // change is not said.
    for withdrawal in &changes.withdrawals {
        let identity = withdrawal.identity();
        let was_there = before
            .get(&identity)
            .map(|supports| supports.iter().any(|one| one == &withdrawal.evidence_id))
            .unwrap_or(false);
        if !was_there {
            continue;
        }
        deltas.push(RelationDelta {
            kind: RelationDeltaKind::Removed,
            tenant: withdrawal.tenant_hrn.clone(),
            relation: withdrawal.relation.clone(),
            from: withdrawal.from.clone(),
            to: withdrawal.to.clone(),
            evidence: withdrawal.evidence_id.clone(),
            cause: cause.to_string(),
            observed_at,
        });
    }

    deltas
}
