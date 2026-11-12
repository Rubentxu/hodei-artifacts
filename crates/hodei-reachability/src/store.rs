//! The store of the reachability evidence.
//!
//! The evidence is **versioned by the analyser**, and the versions are
//! kept: the same source analysed by two versions of an analyser can give
//! two answers, and a reachability that does not name the version cannot
//! be reproduced or argued about.
//!
//! The store is queried **by symbol**, because that is the question: is
//! this symbol of the vulnerable package reached? And the answer comes
//! with the evidence it was derived from, so that a reader can see the
//! coverage and the limits and decide whether to believe it.

use std::sync::{Arc, Mutex};

use crate::derivation::{Reachability, derive};
use crate::evidence::ReachabilityEvidence;
use crate::identity::{CodeIdentity, VulnerableSymbol};

/// Why the evidence could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReachabilityStoreError {
    /// The store is not available.
    Unavailable(String),
}

impl std::fmt::Display for ReachabilityStoreError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) => {
                write!(
                    formatter,
                    "the reachability store is not available: {message}"
                )
            }
        }
    }
}

impl std::error::Error for ReachabilityStoreError {}

/// The store of the evidence of reachability.
#[async_trait::async_trait]
pub trait ReachabilityStore: Send + Sync {
    /// Writes an evidence down.
    ///
    /// Two evidences of the same analyser and the same revision are the
    /// same evidence, and writing one over the other is not a change.
    async fn put(&self, evidence: &ReachabilityEvidence) -> Result<(), ReachabilityStoreError>;

    /// The evidence of a symbol, of a revision.
    async fn evidence_of(
        &self,
        source_revision: &str,
        symbol: &CodeIdentity,
    ) -> Result<Vec<ReachabilityEvidence>, ReachabilityStoreError>;

    /// All the evidence of a revision.
    async fn evidence_of_revision(
        &self,
        source_revision: &str,
    ) -> Result<Vec<ReachabilityEvidence>, ReachabilityStoreError>;
}

/// The store of the evidence, in memory.
///
/// It is the store for a deployment that has not wired the persistent one,
/// and it is the one the contract test exercises.
#[derive(Debug, Clone, Default)]
pub struct InMemoryReachabilityStore {
    evidences: Arc<Mutex<Vec<ReachabilityEvidence>>>,
}

impl InMemoryReachabilityStore {
    /// A store with nothing in it.
    pub fn empty() -> Self {
        Self::default()
    }

    /// The reachability of a symbol, derived from the evidence that
    /// covers it.
    ///
    /// When there is more than one evidence, the one of the **newest
    /// analyser** wins, and the others are kept: the history of what the
    /// analysers said is not thrown away when a new version arrives.
    pub fn reachability_of(
        &self,
        source_revision: &str,
        symbol: &VulnerableSymbol,
    ) -> Result<Option<Reachability>, ReachabilityStoreError> {
        let evidences = self.evidences.lock().expect("the store is not poisoned");

        let mut candidates: Vec<&ReachabilityEvidence> = evidences
            .iter()
            .filter(|evidence| evidence.coverage.source_revision == source_revision)
            .filter(|evidence| evidence.coverage.covers(&symbol.identity))
            .collect();

        if candidates.is_empty() {
            return Ok(None);
        }

        // The newest evidence wins, and the ones before it are still
        // there.
        candidates.sort_by(|left, right| {
            left.analyzed_at
                .cmp(&right.analyzed_at)
                .then(left.analyzer.version.cmp(&right.analyzer.version))
        });

        Ok(Some(derive(
            candidates.last().expect("there is one"),
            &symbol.identity,
        )))
    }

    /// The number of evidences that are stored.
    pub fn len(&self) -> usize {
        self.evidences
            .lock()
            .expect("the store is not poisoned")
            .len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[async_trait::async_trait]
impl ReachabilityStore for InMemoryReachabilityStore {
    async fn put(&self, evidence: &ReachabilityEvidence) -> Result<(), ReachabilityStoreError> {
        let mut evidences = self.evidences.lock().expect("the store is not poisoned");
        evidences.retain(|existing| {
            !(existing.analyzer == evidence.analyzer
                && existing.coverage.source_revision == evidence.coverage.source_revision)
        });
        evidences.push(evidence.clone());
        Ok(())
    }

    async fn evidence_of(
        &self,
        source_revision: &str,
        symbol: &CodeIdentity,
    ) -> Result<Vec<ReachabilityEvidence>, ReachabilityStoreError> {
        Ok(self
            .evidence_of_revision(source_revision)
            .await?
            .into_iter()
            .filter(|evidence| evidence.coverage.covers(symbol))
            .collect())
    }

    async fn evidence_of_revision(
        &self,
        source_revision: &str,
    ) -> Result<Vec<ReachabilityEvidence>, ReachabilityStoreError> {
        let evidences = self.evidences.lock().expect("the store is not poisoned");
        Ok(evidences
            .iter()
            .filter(|evidence| evidence.coverage.source_revision == source_revision)
            .cloned()
            .collect())
    }
}
