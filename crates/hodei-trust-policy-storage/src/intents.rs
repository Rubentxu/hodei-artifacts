//! The durable store of the reactive work, over the SurrealDB that
//! Artifacts already has.
//!
//! It answers for two ports, and it adds no store and no broker:
//!
//! - [`ExecutionStore`](hodei_reactive::runtime::ExecutionStore): the
//!   records and the receipts of the reactive runtime, **durable** -- a
//!   restart reads them again, with their causality, because la memoria
//!   del proceso no es la verdad.
//! - [`IntentOutbox`](hodei_reactive::intent::IntentOutbox): the intents
//!   that are said and not yet done, with the explicit states
//!   pending/claimed/applied/failed/unknown, the lease that lets a dead
//!   worker's intent go back to the market, and the compare-and-set that
//!   keeps two workers from winning the same claim.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hodei_reactive::behavior::Effect;
use hodei_reactive::execution::ExecutionRecord;
use hodei_reactive::intent::{Intent, IntentOutbox, Receipt};
use hodei_reactive::runtime::ExecutionStore;
use serde::{Deserialize, Serialize};
use surrealdb::Connection;
use surrealdb::Surreal;

/// The table the execution records live in.
pub const EXECUTION_TABLE: &str = "reactive_execution";

/// The table the applied effects live in.
pub const EFFECT_TABLE: &str = "reactive_effect";

/// The table the intents wait in.
pub const INTENT_TABLE: &str = "reactive_intent";

/// Escapes a part of a record identifier, with the same rules the graph
/// store uses: lo que llega de fuera no se escribe tal cual en un id.
fn escape_id(value: &str) -> String {
    value.replace('~', "~0").replace(':', "~1")
}

/// The row of an intent: the intent, and the state it is in.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct IntentRow {
    /// pending | claimed | applied | failed | unknown.
    state: String,
    /// The worker that claimed it, while the lease lasts.
    #[serde(default)]
    lease_owner: Option<String>,
    /// When the lease expires.
    #[serde(default)]
    lease_until: Option<DateTime<Utc>>,
    /// What the effect turned out to be, when the receipt came, said as
    /// JSON: un enum de Rust no cruza la frontera de SurrealDB, y su
    /// JSON si, y se lee de vuelta con el mismo serde.
    #[serde(default)]
    receipt: Option<serde_json::Value>,
    /// The intent itself, with everything that makes it reconcilable.
    intent: Intent,
}

/// One row of the operational state, said for the operator and for the
/// agent that asks what is waiting, what was confirmed and why something
/// stopped. It carries NO payload: el payload de un intent es dato, y el
/// diagnostico es para leer el ESTADO, no para re-derivar los hechos --
/// y credenciales no viajan en un diagnostico.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IntentDiagnostic {
    /// The run that planned it: the behaviour's name.
    pub run: String,
    /// The effect it wants, once: the idempotency key.
    pub effect: String,
    /// The tenant the intent serves.
    pub tenant: String,
    /// The event that triggered it.
    pub event_id: String,
    /// The causation chain of the event, oldest first.
    pub cause: Vec<String>,
    /// How many times the effect was attempted, as the record says.
    pub attempts: u32,
    /// pending | claimed | applied | failed | unknown.
    pub state: String,
    /// Why it stopped, when a receipt says so.
    pub failure_reason: Option<String>,
}

/// The durable store, over the database that already exists.
pub struct SurrealIntentStore<C: Connection> {
    db: Arc<Surreal<C>>,
}

impl<C: Connection> SurrealIntentStore<C> {
    /// The store over a database.
    pub fn new(db: Arc<Surreal<C>>) -> Self {
        Self { db }
    }

    /// The identifier of a record, escaped.
    fn record_id(table: &str, key: &str) -> surrealdb::sql::Thing {
        surrealdb::sql::Thing::from((table.to_string(), escape_id(key)))
    }

    /// The identifier of an intent's row.
    fn intent_id(identity: &str) -> surrealdb::sql::Thing {
        Self::record_id(INTENT_TABLE, identity)
    }

    /// The identifier of the row of an execution.
    fn execution_id(key: &str) -> surrealdb::sql::Thing {
        Self::record_id(EXECUTION_TABLE, key)
    }

    /// The identifier of the row of an effect.
    fn effect_id(key: &str) -> surrealdb::sql::Thing {
        Self::record_id(EFFECT_TABLE, key)
    }

    /// Every intent and its state, sorted by identity, for the reader
    /// that asks what is waiting and what became of what was said.
    pub async fn states(&self) -> Result<Vec<(String, String)>, String> {
        let rows: Vec<IntentRow> = self
            .db
            .query("SELECT * FROM type::table($table)")
            .bind(("table", INTENT_TABLE))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        let mut states: Vec<(String, String)> = rows
            .into_iter()
            .map(|row| (row.intent.identity, row.state))
            .collect();
        states.sort();
        Ok(states)
    }

    /// The operational state of the intents of ONE tenant, said for the
    /// operator and for the agent that asks what is waiting, what was
    /// confirmed and why something stopped. The read is tenant-scoped:
    /// the caller names the tenant it serves, and NOTHING outside it is
    /// returned -- una vista que contesta de mas es una vista que
    /// filtra. Whoever exposes this view to the outside gates it behind
    /// a read authorization; the view itself only knows whose eyes it
    /// answers.
    ///
    /// It carries NO payload: the payload of an intent is data, and the
    /// diagnostic is for reading the STATE, not for re-deriving the
    /// facts.
    pub async fn diagnose(&self, tenant: &str) -> Result<Vec<IntentDiagnostic>, String> {
        if tenant.trim().is_empty() {
            return Err("the diagnostic does not say which tenant it serves".to_string());
        }

        let rows: Vec<IntentRow> = self
            .db
            .query("SELECT * FROM type::table($table) WHERE intent.tenant = $tenant")
            .bind(("table", INTENT_TABLE))
            .bind(("tenant", tenant.to_string()))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;

        let mut diagnostics: Vec<IntentDiagnostic> = Vec::new();
        for row in rows {
            // The receipt says why, when a receipt says anything.
            let failure_reason = row.receipt.as_ref().and_then(|receipt| {
                serde_json::from_value::<Receipt>(receipt.clone())
                    .ok()
                    .and_then(|receipt| match receipt {
                        Receipt::Failed { reason, .. } => Some(reason),
                        Receipt::Unknown { reason } => Some(reason),
                        Receipt::Applied => None,
                    })
            });

            // The attempts, as the record of the apply says, when there
            // is one.
            let attempts = match self
                .db
                .query("SELECT * FROM $id")
                .bind(("id", Self::record_id(
                    EXECUTION_TABLE,
                    &format!("apply:{}", row.intent.identity),
                )))
                .await
            {
                Ok(mut response) => response
                    .take::<Vec<ExecutionRecord>>(0)
                    .map(|records| {
                        records.first().map(|record| record.attempts).unwrap_or(0)
                    })
                    .unwrap_or(0),
                Err(_) => 0,
            };

            diagnostics.push(IntentDiagnostic {
                run: row.intent.origin.behavior.clone(),
                effect: row.intent.idempotency_key.clone(),
                tenant: row.intent.tenant.clone(),
                event_id: row.intent.origin.event_id.clone(),
                cause: row.intent.origin.causation.clone(),
                attempts,
                state: row.state,
                failure_reason,
            });
        }
        diagnostics.sort_by(|left, right| {
            left.run
                .cmp(&right.run)
                .then_with(|| left.effect.cmp(&right.effect))
        });
        Ok(diagnostics)
    }
}

#[async_trait]
impl<C: Connection> ExecutionStore for SurrealIntentStore<C> {
    async fn execution_of(&self, key: &str) -> Option<ExecutionRecord> {
        let rows: Vec<ExecutionRecord> = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::execution_id(key)))
            .await
            .ok()?
            .take(0)
            .ok()?;
        rows.into_iter().next()
    }

    async fn put(&self, record: ExecutionRecord) {
        let _ = self
            .db
            .query("UPSERT $id CONTENT $record")
            .bind(("id", Self::execution_id(&record.key)))
            .bind(("record", record))
            .await;
    }

    async fn applied_effects(&self) -> Vec<Effect> {
        self.db
            .query("SELECT * FROM type::table($table)")
            .bind(("table", EFFECT_TABLE))
            .await
            .and_then(|mut response| response.take(0))
            .unwrap_or_default()
    }

    async fn record_effect(&self, effect: Effect) {
        let _ = self
            .db
            .query("UPSERT $id CONTENT $effect")
            .bind(("id", Self::effect_id(&effect.idempotency_key)))
            .bind(("effect", effect))
            .await;
    }

    async fn executions(&self) -> Vec<ExecutionRecord> {
        let mut records: Vec<ExecutionRecord> = self
            .db
            .query("SELECT * FROM type::table($table)")
            .bind(("table", EXECUTION_TABLE))
            .await
            .and_then(|mut response| response.take(0))
            .unwrap_or_default();
        records.sort_by_key(|record| record.started_at);
        records
    }
}

#[async_trait]
impl<C: Connection> IntentOutbox for SurrealIntentStore<C> {
    async fn put_pending(&self, intent: &Intent) -> Result<(), String> {
        // Saying it twice is the same as saying it once: si el intent ya
        // esta, lo que hay se queda, porque un intent dicho dos veces no
        // es dos intents.
        let id = Self::intent_id(&intent.identity);
        let exists: Vec<IntentRow> = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", id.clone()))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        if !exists.is_empty() {
            return Ok(());
        }

        let row = IntentRow {
            state: "pending".to_string(),
            lease_owner: None,
            lease_until: None,
            receipt: None,
            intent: intent.clone(),
        };
        self.db
            .query("UPSERT $id CONTENT $row")
            .bind(("id", id))
            .bind(("row", row))
            .await
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    async fn claim(
        &self,
        identity: &str,
        lease_owner: &str,
        lease_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<Option<Intent>, String> {
        // The compare-and-set is ONE statement, and the server answers
        // with the rows it actually moved: dos workers piden lo mismo y
        // el que llega segundo ve que ya no hay nada que reclamar.
        let moved: Vec<IntentRow> = self
            .db
            .query(
                "UPDATE $id SET \
                 state = 'claimed', lease_owner = $owner, lease_until = $until \
                 WHERE state = 'pending' OR (state = 'claimed' AND lease_until <= $now)",
            )
            .bind(("id", Self::intent_id(identity)))
            .bind(("owner", lease_owner.to_string()))
            .bind(("until", lease_until))
            .bind(("now", now))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;

        Ok(moved.into_iter().next().map(|row| row.intent))
    }

    async fn record_receipt(
        &self,
        identity: &str,
        receipt: &Receipt,
    ) -> Result<(), String> {
        // A receipt only answers a claim: el que lo aplico es el que lo
        // reclamo, y un recibo para un intent que nadie reclamo no lo
        // puede explicar nadie. El fallo que se puede reintentar vuelve
        // al mercado solo; los demas se quedan donde el recibo dice.
        let receipt_json = serde_json::to_value(receipt).map_err(|error| error.to_string())?;

        // The receipt that says what is ALREADY true changes nothing: el
        // mismo recibo dos veces es un recibo una vez.
        let current: Vec<IntentRow> = self
            .db
            .query("SELECT * FROM $id")
            .bind(("id", Self::intent_id(identity)))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        if let Some(row) = current.first() {
            let already_true = match receipt {
                Receipt::Applied => row.state == "applied",
                Receipt::Failed { retryable: true, .. } => row.state == "pending",
                Receipt::Failed { .. } => row.state == "failed",
                Receipt::Unknown { .. } => row.state == "unknown",
            };
            if already_true {
                return Ok(());
            }
        }

        let sql = match receipt {
            Receipt::Applied => {
                "UPDATE $id SET state = 'applied', receipt = $receipt \
                 WHERE state = 'claimed'"
            }
            Receipt::Failed { retryable: true, .. } => {
                "UPDATE $id SET state = 'pending', receipt = $receipt, \
                 lease_owner = NONE, lease_until = NONE \
                 WHERE state = 'claimed'"
            }
            Receipt::Failed { .. } => {
                "UPDATE $id SET state = 'failed', receipt = $receipt, \
                 lease_owner = NONE, lease_until = NONE \
                 WHERE state = 'claimed'"
            }
            Receipt::Unknown { .. } => {
                "UPDATE $id SET state = 'unknown', receipt = $receipt, \
                 lease_owner = NONE, lease_until = NONE \
                 WHERE state = 'claimed'"
            }
        };

        let moved: Vec<IntentRow> = self
            .db
            .query(sql)
            .bind(("id", Self::intent_id(identity)))
            .bind(("receipt", receipt_json))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        if moved.is_empty() {
            return Err(format!(
                "the intent {identity} is not claimed, so nobody can receive this receipt"
            ));
        }
        Ok(())
    }

    async fn recover_pending(&self, now: DateTime<Utc>) -> Result<Vec<Intent>, String> {
        // The leases that died go back to the market, and the pending are
        // listed. Lo desconocido NO esta aqui: no se reejecuta, se
        // reconcilia.
        self.db
            .query(
                "UPDATE type::table($table) SET state = 'pending', \
                 lease_owner = NONE, lease_until = NONE \
                 WHERE state = 'claimed' AND lease_until <= $now",
            )
            .bind(("table", INTENT_TABLE))
            .bind(("now", now))
            .await
            .map_err(|error| error.to_string())?
            .check()
            .map_err(|error| error.to_string())?;

        let mut rows: Vec<IntentRow> = self
            .db
            .query("SELECT * FROM type::table($table) WHERE state = 'pending'")
            .bind(("table", INTENT_TABLE))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        rows.sort_by(|left, right| left.intent.identity.cmp(&right.intent.identity));
        Ok(rows.into_iter().map(|row| row.intent).collect())
    }

    async fn reconcile(&self, identity: &str) -> Result<(), String> {
        let moved: Vec<IntentRow> = self
            .db
            .query(
                "UPDATE $id SET state = 'pending', receipt = NONE, \
                 lease_owner = NONE, lease_until = NONE \
                 WHERE state = 'unknown'",
            )
            .bind(("id", Self::intent_id(identity)))
            .await
            .map_err(|error| error.to_string())?
            .take(0)
            .map_err(|error| error.to_string())?;
        if moved.is_empty() {
            return Err(format!(
                "the intent {identity} is not unknown, so there is nothing to reconcile"
            ));
        }
        Ok(())
    }
}
