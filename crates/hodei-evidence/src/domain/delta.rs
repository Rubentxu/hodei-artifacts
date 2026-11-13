//! The deltas of the relations of the risk graph.
//!
//! When a support is added, withdrawn or changed, what changed is a
//! FACT about one relation: el producto lo reconoce y reacciona a ello
//! sin recorrer ni recalcular el grafo entero. The delta is said once,
//! with its cause and its version, and who invalidates reads it from
//! where it stopped -- a restart does not lose what was said, and
//! saying it twice does not say it twice.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::evidence::EvidenceId;
use crate::domain::graph::RelationType;
use kernel::Hrn;

/// What a delta says happened to a relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelationDeltaKind {
    /// The relation gained its first support: una afirmacion nueva.
    Added,
    /// The relation lost a support. It may still hold others: la
    /// retirada de UN apoyo no es la retirada de la relacion.
    Removed,
    /// The relation already held supports and gained one more.
    SupportChanged,
}

impl RelationDeltaKind {
    /// The name of the kind, as it is said.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::SupportChanged => "support_changed",
        }
    }
}

/// One change of one risk relation, said for the ones that invalidate.
///
/// It carries WHAT changed, WHERE, WHOSE evidence moved it, WHAT caused
/// it and WHERE it stands in the tenant's order. La causa es la
/// identidad de lo que lo trajo -- el evento o el changeset -- y es la
/// clave de la deduplicacion: el mismo cambio dicho dos veces es UN
/// cambio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationDelta {
    /// What happened to the relation.
    pub kind: RelationDeltaKind,
    /// The account the relation belongs to.
    pub tenant: Hrn,
    /// The type of the relation.
    pub relation: RelationType,
    /// The node the relation starts at.
    pub from: Hrn,
    /// The node the relation ends at.
    pub to: Hrn,
    /// The evidence whose support moved.
    pub evidence: EvidenceId,
    /// What caused it: the identity of the event or changeset that
    /// brought it. Saying the same cause twice is the same delta.
    pub cause: String,
    /// When it was observed.
    pub observed_at: DateTime<Utc>,
}

impl RelationDelta {
    /// The identity of the relation the delta is about: the same
    /// identity the relation and the withdrawal carry.
    pub fn identity(&self) -> String {
        format!("{}|{}|{}", self.relation.as_str(), self.from, self.to)
    }
}

/// What a subscriber of the deltas wants: SU tenant, and a CLOSED set of
/// relation types. A delta of another account or of a relation type
/// outside the set is not this subscriber's business, and it does not
/// invalidate anything of theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationDeltaSubscription {
    /// The tenant whose deltas are wanted.
    pub tenant: Hrn,
    /// The relation types the subscriber invalidates for.
    pub relations: BTreeSet<RelationType>,
}

impl RelationDeltaSubscription {
    /// A subscription of one tenant for a closed set of relation types.
    pub fn new(tenant: Hrn, relations: impl IntoIterator<Item = RelationType>) -> Self {
        Self {
            tenant,
            relations: relations.into_iter().collect(),
        }
    }

    /// Whether the delta is this subscriber's business.
    pub fn wants(&self, delta: &RelationDelta) -> bool {
        self.tenant == delta.tenant && self.relations.contains(&delta.relation)
    }
}

/// The ledger of the deltas of the relations.
///
/// The deltas are PERSISTED, with their version --the order they take in
/// the tenant's story-- and their cause. A consumer reads from where it
/// stopped, within a quota, and a restart reads the same deltas again:
/// lo que se dijo sigue dicho. The append is idempotent by cause: the
/// same cause appended twice is ONE delta, not two.
#[async_trait]
pub trait RelationDeltaLedger: Send + Sync {
    /// Appends deltas, in order, and answers with the version of the
    /// LAST one appended. Deltas whose cause is already in the ledger
    /// are not appended again.
    async fn append(&self, deltas: &[RelationDelta]) -> Result<Option<u64>, String>;

    /// The deltas a subscriber wants, after a version, within a quota.
    /// The answer says where the reader stopped, so that the next read
    /// continues there and nothing is read twice.
    async fn pending(
        &self,
        subscription: &RelationDeltaSubscription,
        after_version: u64,
        quota: usize,
    ) -> Result<(Vec<(u64, RelationDelta)>, u64), String>;

    /// The version the tenant's story has reached: donde el consumidor
    /// nuevo empieza a leer.
    async fn version_of(&self, tenant: &Hrn) -> Result<u64, String>;
}

/// The ledger of the deltas, in memory.
///
/// The reference material of the port: the same rules the durable one
/// keeps -- the order per tenant, the dedup by cause, the quota -- held
/// in the process that holds its own memory.
#[derive(Debug, Clone, Default)]
pub struct InMemoryRelationDeltaLedger {
    inner: Arc<Mutex<DeltaInner>>,
}

#[derive(Debug, Default)]
struct DeltaInner {
    /// The deltas, in the order they were appended, with their version.
    deltas: Vec<(u64, RelationDelta)>,
    /// The causes that are already said, per tenant: el mismo cambio
    /// dicho dos veces es UN cambio.
    causes: BTreeMap<String, BTreeSet<String>>,
}

impl InMemoryRelationDeltaLedger {
    /// A ledger with nothing in it.
    pub fn empty() -> Self {
        Self::default()
    }
}

#[async_trait]
impl RelationDeltaLedger for InMemoryRelationDeltaLedger {
    async fn append(&self, deltas: &[RelationDelta]) -> Result<Option<u64>, String> {
        let mut inner = self.inner.lock().expect("el registro no esta envenenado");
        let mut last = None;
        for delta in deltas {
            // The key of the dedup: the cause AND what it said. El mismo
            // cambio dicho dos veces es UN cambio; otro cambio del mismo
            // cause es otro cambio.
            let key = format!(
                "{}|{}|{}|{}",
                delta.cause,
                delta.kind.as_str(),
                delta.identity(),
                delta.evidence.as_str()
            );
            let causes = inner
                .causes
                .entry(delta.tenant.to_string())
                .or_default();
            if !causes.insert(key) {
                continue;
            }
            let version = inner
                .deltas
                .iter()
                .filter(|(_version, said)| said.tenant == delta.tenant)
                .map(|(version, _)| *version)
                .max()
                .unwrap_or(0)
                + 1;
            inner.deltas.push((version, delta.clone()));
            last = Some(version);
        }
        Ok(last)
    }

    async fn pending(
        &self,
        subscription: &RelationDeltaSubscription,
        after_version: u64,
        quota: usize,
    ) -> Result<(Vec<(u64, RelationDelta)>, u64), String> {
        let inner = self.inner.lock().expect("el registro no esta envenenado");
        let mut wanted: Vec<(u64, RelationDelta)> = Vec::new();
        for (version, delta) in &inner.deltas {
            if *version <= after_version {
                continue;
            }
            if !subscription.wants(delta) {
                continue;
            }
            if wanted.len() >= quota {
                break;
            }
            wanted.push((*version, delta.clone()));
        }
        // Where the reader stopped: la version del ultimo que leyo, y si
        // no leyo ninguno, la que trajo.
        let stopped = wanted
            .last()
            .map(|(version, _)| *version)
            .unwrap_or(after_version);
        Ok((wanted, stopped))
    }

    async fn version_of(&self, tenant: &Hrn) -> Result<u64, String> {
        let inner = self.inner.lock().expect("el registro no esta envenenado");
        Ok(inner
            .deltas
            .iter()
            .filter(|(_version, delta)| delta.tenant == *tenant)
            .map(|(version, _)| *version)
            .max()
            .unwrap_or(0))
    }
}
