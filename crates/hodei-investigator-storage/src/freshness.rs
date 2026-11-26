//! The freshness checker, over the storage that already exists.
//!
//! The verifier --the pure function-- lives in the domain
//! (`hodei_investigator::freshness`). Here the only thing done is to **fetch the world**: the
//! current version, the current digests and the current changes. The storage
//! does not decide, and this separation is what makes the
//! verdict the same with any material.
//!
//! # No subscription engine
//!
//! The check is **on demand**: whoever holds the report asks,
//! and the answer is computed with what is there. There is no engine pushing
//! invalidations, nor a queue, nor a new table: the delta log that
//! already exists is the history, and it is read from where it was left.
//!
//! # Tenant isolation
//!
//! The log is queried **by tenant**, and the tenant is that of the
//! account being checked: a query from another account does not reach
//! the log. And the subject has to **belong to the tenant**: a subject of
//! another account is not queried, and it is reported as unknown instead of
//! querying whatever.

use std::collections::BTreeSet;
use std::sync::Arc;

use hodei_evidence::domain::delta::{
    RelationDeltaKind, RelationDeltaLedger, RelationDeltaSubscription,
};
use hodei_evidence::domain::graph::RelationType;
use hodei_evidence::ports::EvidenceStore;
use hodei_investigator::freshness::{
    ChangeKind, ChangedReference, CurrentWorld, EVIDENCE_SOURCE, Freshness, GRAPH_SOURCE, judge,
};
use hodei_investigator::read_set::{ReadKind, ReadSet};
use kernel::Hrn;

/// How many deltas are read at a time.
const PAGE: usize = 512;

/// How many pages are read before giving up.
///
/// The limit is a **work** gate, not a time one: reading the deltas
/// of an account that changed a lot is bounded work, and if it does not fit
/// within the limit the answer is not "current" but "unknown".
const MAX_PAGES: usize = 64;

/// The freshness checker over the graph and the evidence.
///
/// The tenant is that of the account being checked, and the queries it
/// makes --the delta log and the subject's evidence-- belong to that
/// tenant and to no other.
pub struct LedgerFreshness {
    evidence: Arc<dyn EvidenceStore>,
    deltas: Arc<dyn RelationDeltaLedger>,
    tenant: Hrn,
}

impl LedgerFreshness {
    /// The checker over the evidence store and the delta
    /// log, for a tenant.
    pub fn new(
        evidence: Arc<dyn EvidenceStore>,
        deltas: Arc<dyn RelationDeltaLedger>,
        tenant: Hrn,
    ) -> Self {
        Self {
            evidence,
            deltas,
            tenant,
        }
    }

    /// The freshness of the report of a read.
    ///
    /// The manifest is that of the delivered report, and the subject is the
    /// same subject the report states. A subject of another account is not
    /// queried: the verdict is unknown, not an answer about
    /// what is not theirs.
    pub async fn freshness_of(&self, report: &ReadSet, subject: &Hrn) -> Freshness {
        if subject.account_id() != self.tenant.account_id() {
            return Freshness::Unknown {
                reason: "the subject does not belong to the tenant of the check".to_string(),
            };
        }
        let world = self.the_world(report, subject).await;
        judge(report, &world)
    }

    /// The current world, for what the manifest says it read.
    async fn the_world(&self, report: &ReadSet, subject: &Hrn) -> CurrentWorld {
        let mut world = CurrentWorld::default();

        // The graph version: the tenant's position in its history.
        match self.deltas.version_of(&self.tenant).await {
            Ok(version) => world.generations.push((GRAPH_SOURCE.to_string(), version)),
            Err(error) => {
                world.unreachable.push(GRAPH_SOURCE.to_string());
                tracing::warn!(%error, "the version of the graph could not be read");
            }
        }

        // The changes since the version the manifest says it
        // read. If the version did not move, there is nothing to look at.
        if let Some(after) = the_read_version(report, GRAPH_SOURCE)
            && let Some((_, current)) = world
                .generations
                .iter()
                .find(|(source, _)| source == GRAPH_SOURCE)
            && *current != after
        {
            match self.the_changes(report, after).await {
                Ok(changes) => world.changes = changes,
                Err(error) => {
                    world.unreachable.push(GRAPH_SOURCE.to_string());
                    tracing::warn!(%error, "the changes of the graph could not be read");
                }
            }
        }

        // The digests of the evidence that exists now: what was read is
        // checked against what is there.
        match self
            .evidence
            .evidence_of_subject(self.tenant.account_id(), subject)
            .await
        {
            Ok(records) => {
                for record in records {
                    world
                        .digests
                        .push((record.id().to_string(), record.content().digest.to_string()));
                }
            }
            Err(error) => {
                world.unreachable.push(EVIDENCE_SOURCE.to_string());
                tracing::warn!(%error, "the evidence of the subject could not be read");
            }
        }

        world
    }

    /// The graph changes since the version the manifest says it
    /// read.
    ///
    /// They are read by pages until reaching the current version, and if they do not
    /// fit within the work limit the answer is unknown: a
    /// "current" without having read everything would be an invented guarantee.
    async fn the_changes(
        &self,
        report: &ReadSet,
        after: u64,
    ) -> Result<Vec<ChangedReference>, String> {
        let Some(types) = the_relation_types(report) else {
            return Err("the relations of the report name a type that is not known".to_string());
        };
        let subscription = RelationDeltaSubscription::new(self.tenant.clone(), types);
        let mut changes: Vec<ChangedReference> = Vec::new();
        let mut cursor = after;
        for _ in 0..MAX_PAGES {
            let (deltas, stopped) = self.deltas.pending(&subscription, cursor, PAGE).await?;
            for (_, delta) in &deltas {
                let reference = ChangedReference {
                    source: GRAPH_SOURCE.to_string(),
                    kind: ReadKind::Relation,
                    resource: delta.identity(),
                    evidence: delta.evidence.to_string(),
                    what: match delta.kind {
                        RelationDeltaKind::Added => ChangeKind::Added,
                        RelationDeltaKind::Removed => ChangeKind::Removed,
                        RelationDeltaKind::SupportChanged => ChangeKind::Changed,
                    },
                };
                if !changes.contains(&reference) {
                    changes.push(reference);
                }
            }
            if deltas.len() < PAGE || stopped <= cursor {
                // There are no more deltas to read: the end was reached.
                return Ok(changes);
            }
            cursor = stopped;
        }
        Err("the changes of the graph did not fit in the work of the check".to_string())
    }
}

/// The version the manifest says it read from a source.
fn the_read_version(report: &ReadSet, source: &str) -> Option<u64> {
    report
        .generations
        .iter()
        .find(|(name, _)| name == source)
        .map(|(_, version)| *version)
}

/// The relation types the manifest says it read.
///
/// They are read from the manifest itself: the identity of a relation's seal
/// is "{type}|{from}|{to}", and what the report relies on is its
/// types. Asking about more would be asking about what is not the
/// report's, and asking about fewer would be missing the changes that
/// invalidate it.
fn the_relation_types(report: &ReadSet) -> Option<BTreeSet<RelationType>> {
    let mut types = BTreeSet::new();
    for stamp in report
        .stamps
        .iter()
        .filter(|s| s.kind == ReadKind::Relation)
    {
        let name = stamp.resource.split('|').next()?;
        types.insert(RelationType::parse(name)?);
    }
    Some(types)
}
