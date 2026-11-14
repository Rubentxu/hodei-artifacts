//! The ledger of the relation deltas, over the store that already
//! exists.
//!
//! The deltas are PERSISTED with their version --el orden que toman en
//! la historia del tenant-- and their cause. A consumer reads from
//! where it stopped, within a quota, and a restart reads the same
//! deltas again. The append is idempotent: el mismo cambio dicho dos
//! veces es UN cambio, y su registro lleva la causa y lo que dijo.

use std::collections::BTreeSet;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_evidence::domain::delta::{
    RelationDelta, RelationDeltaKind, RelationDeltaLedger, RelationDeltaSubscription,
};
use serde::{Deserialize, Serialize};
use surrealdb::Connection;
use surrealdb::Surreal;

/// The table the deltas live in. It is NOT part of a projection: los
/// deltas son hechos operativos, y sobreviven a una reconstruccion del
/// grafo.
pub const DELTA_TABLE: &str = "relation_delta";

/// The row of a delta, as it is stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaRow {
    /// The version of the delta in the tenant's story.
    pub seq: u64,
    /// The account the delta belongs to.
    pub tenant: String,
    /// added | removed | support_changed.
    pub kind: String,
    /// The type of the relation.
    pub relation: String,
    /// The node the relation starts at.
    pub from: String,
    /// The node the relation ends at.
    pub to: String,
    /// The evidence whose support moved.
    pub evidence: String,
    /// What caused it.
    pub cause: String,
    /// When it was observed.
    pub observed_at: DateTime<Utc>,
}

/// The durable ledger of the deltas, over the database that exists.
pub struct SurrealRelationDeltaLedger<C: Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: Connection> SurrealRelationDeltaLedger<C> {
    /// The ledger over a database.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }

    /// The identifier of a delta's record: the cause AND what it said,
    /// so the same change said twice is the same record.
    fn record_id(delta: &RelationDelta) -> surrealdb::sql::Thing {
        let key = format!(
            "{}|{}|{}|{}|{}",
            delta.tenant,
            delta.cause,
            delta.kind.as_str(),
            delta.identity(),
            delta.evidence.as_str()
        );
        surrealdb::sql::Thing::from((DELTA_TABLE.to_string(), escape_id(&key)))
    }

    /// The version the tenant's story has reached, said as the biggest
    /// sequence it holds.
    async fn the_max_seq(&self, tenant: &str) -> Result<u64, String> {
        let rows: Vec<DeltaRow> = self
            .db
            .query("SELECT * FROM type::table($table) WHERE tenant = $tenant ORDER BY seq DESC LIMIT 1")
            .bind(("table", DELTA_TABLE))
            .bind(("tenant", tenant.to_string()))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        Ok(rows.first().map(|row| row.seq).unwrap_or(0))
    }
}

/// Escapes a part of a record identifier, with the same rules the graph
/// store uses.
fn escape_id(value: &str) -> String {
    value.replace('~', "~0").replace(':', "~1")
}

fn row_of(seq: u64, delta: &RelationDelta) -> DeltaRow {
    DeltaRow {
        seq,
        tenant: delta.tenant.to_string(),
        kind: delta.kind.as_str().to_string(),
        relation: delta.relation.as_str().to_string(),
        from: delta.from.to_string(),
        to: delta.to.to_string(),
        evidence: delta.evidence.as_str().to_string(),
        cause: delta.cause.clone(),
        observed_at: delta.observed_at,
    }
}

fn delta_of(row: &DeltaRow) -> Option<RelationDelta> {
    let tenant = kernel::Hrn::from_string(&row.tenant)?;
    let from = kernel::Hrn::from_string(&row.from)?;
    let to = kernel::Hrn::from_string(&row.to)?;
    let relation = hodei_evidence::RelationType::parse(&row.relation)?;
    let kind = match row.kind.as_str() {
        "added" => RelationDeltaKind::Added,
        "removed" => RelationDeltaKind::Removed,
        "support_changed" => RelationDeltaKind::SupportChanged,
        _ => return None,
    };
    Some(RelationDelta {
        kind,
        tenant,
        relation,
        from,
        to,
        evidence: hodei_evidence::EvidenceId::new(row.evidence.clone()),
        cause: row.cause.clone(),
        observed_at: row.observed_at,
    })
}

#[async_trait]
impl<C: Connection> RelationDeltaLedger for SurrealRelationDeltaLedger<C> {
    async fn append(&self, deltas: &[RelationDelta]) -> Result<Option<u64>, String> {
        // The dedup runs BEFORE the writes: lo que ya esta dicho no se
        // dice otra vez, y no consume version alguna. The lookups are
        // reads by record identity, one per candidate.
        let mut fresh: Vec<(u64, &RelationDelta)> = Vec::new();
        let mut seq = 0;
        for delta in deltas {
            let existing: Vec<DeltaRow> = self
                .db
                .query("SELECT * FROM $id")
                .bind(("id", Self::record_id(delta)))
                .await
                .map_err(|error| error.to_string())?
                .take(0)
                .map_err(|error| error.to_string())?;
            if !existing.is_empty() {
                continue;
            }
            if seq == 0 {
                seq = self.the_max_seq(&delta.tenant.to_string()).await?;
            }
            seq += 1;
            fresh.push((seq, delta));
        }
        if fresh.is_empty() {
            return Ok(None);
        }

        // The writes are ONE transaction: los deltas de un changeset
        // entran todos o no entra ninguno.
        let mut sql = String::from("BEGIN TRANSACTION;\n");
        for index in 0..fresh.len() {
            sql.push_str(&format!("UPSERT $id{index} CONTENT $row{index};\n"));
        }
        sql.push_str("COMMIT;\n");

        let mut statement = self.db.query(sql);
        for (index, (seq, delta)) in fresh.iter().enumerate() {
            statement = statement
                .bind((format!("id{index}"), Self::record_id(delta)))
                .bind((format!("row{index}"), row_of(*seq, delta)));
        }
        statement
            .await
            .map_err(|error| error.to_string())?
            .check()
            .map_err(|error| error.to_string())?;

        Ok(fresh.last().map(|(seq, _)| *seq))
    }

    async fn pending(
        &self,
        subscription: &RelationDeltaSubscription,
        after_version: u64,
        quota: usize,
    ) -> Result<(Vec<(u64, RelationDelta)>, u64), String> {
        // A subscription with an EMPTY set of relation types wants
        // nothing: una suscripcion cerrada que no nombra tipos no
        // invalida nada.
        if subscription.relations.is_empty() || quota == 0 {
            return Ok((Vec::new(), after_version));
        }
        let types: Vec<String> = subscription
            .relations
            .iter()
            .map(|relation| relation.as_str().to_string())
            .collect();
        let mut rows: Vec<DeltaRow> = self
            .db
            .query(
                "SELECT * FROM type::table($table) \
                 WHERE tenant = $tenant AND relation IN $types AND seq > $after \
                 ORDER BY seq ASC LIMIT $quota",
            )
            .bind(("table", DELTA_TABLE))
            .bind(("tenant", subscription.tenant.to_string()))
            .bind(("types", types))
            .bind(("after", after_version))
            .bind(("quota", quota as i64))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        rows.sort_by_key(|row| row.seq);
        let wanted: Vec<(u64, RelationDelta)> = rows
            .iter()
            .filter_map(|row| delta_of(row).map(|delta| (row.seq, delta)))
            .collect();
        let stopped = wanted.last().map(|(seq, _)| *seq).unwrap_or(after_version);
        Ok((wanted, stopped))
    }

    async fn version_of(&self, tenant: &kernel::Hrn) -> Result<u64, String> {
        self.the_max_seq(&tenant.to_string()).await
    }
}

/// A set of causes, said the same way the ledger says them.
pub type KnownCauses = BTreeSet<String>;
