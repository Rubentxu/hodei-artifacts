//! The reader of the tenant's delta story, over the frontier.
//!
//! HSP-89 proved, on the Surreal reference, that the freshness of a
//! delivered report is answered by the SAME history the projection
//! writes: the version the manifest says it read is looked up against
//! the version the story reached, and what moved between the two is
//! read page by page. This adapter is that port over Dgraph: it reads
//! the deltas the frontier committed — there is no second ledger, no
//! mirror, no place where a retired support could hide from the check.
//!
//! The frontier is write-through for the projections that deliver
//! events: `append` is refused here on purpose. A delta that was not
//! said by a committed frontier delivery is not part of the story, and
//! an adapter that let a caller whisper one would break the promise
//! the read-set is built on — that the manifest verifies against the
//! history the graph itself wrote.

use std::sync::Arc;

use async_trait::async_trait;
use hodei_evidence::domain::delta::{
    RelationDelta, RelationDeltaLedger, RelationDeltaSubscription,
};
use kernel::Hrn;

use super::frontier::DgraphProjectionFrontier;

/// The delta history of the frontier, behind the port of the readers.
///
/// It is a reader only: the version of the story and the deltas after
/// a cursor, the same two questions the Surreal ledger answers for the
/// freshness check and the reactions.
pub struct DgraphDeltaLedger {
    frontier: Arc<DgraphProjectionFrontier>,
}

impl DgraphDeltaLedger {
    /// The reader over an open frontier. The frontier carries the
    /// tenant namespace its checkpoints live under; the deltas are
    /// asked by the subscription's tenant, exactly as the frontier
    /// stored them.
    pub fn new(frontier: Arc<DgraphProjectionFrontier>) -> Self {
        Self { frontier }
    }
}

#[async_trait]
impl RelationDeltaLedger for DgraphDeltaLedger {
    /// Refused: the deltas of this story are said by the frontier's
    /// committed deliveries, not appended by whoever asks a question.
    /// A write here would be a fact the graph never projected.
    async fn append(&self, _deltas: &[RelationDelta]) -> Result<Option<u64>, String> {
        Err("the delta story of the graph is written by the frontier, not appended".to_string())
    }

    async fn pending(
        &self,
        subscription: &RelationDeltaSubscription,
        after_version: u64,
        quota: usize,
    ) -> Result<(Vec<(u64, RelationDelta)>, u64), String> {
        self.frontier
            .pending_deltas(subscription, after_version, quota)
            .await
            .map_err(|error| error.to_string())
    }

    async fn version_of(&self, tenant: &Hrn) -> Result<u64, String> {
        self.frontier
            .delta_version(tenant)
            .await
            .map_err(|error| error.to_string())
    }
}
