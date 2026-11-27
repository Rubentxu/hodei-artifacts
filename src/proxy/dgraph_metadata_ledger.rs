//! The durable, append-only ledger of accepted mirror metadata over the graph
//! authority (HSP-167). It persists what [`super::metadata_guard`] decides on.
//!
//! Each step of a lineage is one row keyed `{account}/{repository}/
//! {distribution}#{seq}`; the key predicate is `@upsert`, and the write is
//! create-if-absent. Two writers that read the same head both try to create
//! row `seq + 1`: the server lets one win and the other either finds the key
//! standing (the condition skips the write) or is aborted with a typed
//! conflict; either way it reports "not written" and re-decides over the new
//! head. Nothing is rewritten or deleted, so the history of accepted `Date`s
//! is kept and a restart loses nothing.
//!
//! The slice declares its own additive predicates (`hodei.mirrormeta.*`); it
//! never touches the versioned graph schema.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use hodei_graph_client::GraphClient;
use hodei_graph_client::port::{GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction};
use hodei_graph_client::schema::dql_string;
use serde_json::Value;
use std::sync::Arc;

use super::metadata_guard::{Baseline, Head, LedgerError, MetadataKey, MirrorMetadataLedger, Step};

/// The type name of one ledger row.
pub const ROW_TYPE: &str = "HodeiMirrorMetadata";

/// The additive DDL slice of the ledger. Idempotent.
pub const MIRROR_METADATA_SCHEMA_DQL: &str = "
hodei.mirrormeta.key: string @index(exact) @upsert .
hodei.mirrormeta.subject: string @index(exact) .
hodei.mirrormeta.seq: int @index(int) .
hodei.mirrormeta.kind: string .
hodei.mirrormeta.date: int .
hodei.mirrormeta.digest: string .
hodei.mirrormeta.at: string .
hodei.mirrormeta.actor: string .
type HodeiMirrorMetadata {
  hodei.mirrormeta.key
  hodei.mirrormeta.subject
  hodei.mirrormeta.seq
  hodei.mirrormeta.kind
  hodei.mirrormeta.date
  hodei.mirrormeta.digest
  hodei.mirrormeta.at
  hodei.mirrormeta.actor
}
";

/// The ledger over a connected Dgraph client.
pub struct DgraphMirrorMetadataLedger {
    client: Arc<GraphClient>,
}

impl std::fmt::Debug for DgraphMirrorMetadataLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DgraphMirrorMetadataLedger")
            .finish_non_exhaustive()
    }
}

fn unavailable(detail: impl ToString) -> LedgerError {
    LedgerError(detail.to_string())
}

/// A server-side refusal travels inside the JSON and is never a success.
fn require_no_errors(json: &Value) -> Result<(), LedgerError> {
    match json.get("errors").and_then(Value::as_array) {
        Some(errors) if !errors.is_empty() => Err(unavailable(format!("{errors:?}"))),
        _ => Ok(()),
    }
}

impl DgraphMirrorMetadataLedger {
    /// Opens the ledger: declares the additive DDL slice, bounded and
    /// idempotent.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, LedgerError> {
        let mut last = String::new();
        for attempt in 1..=3 {
            match client.alter_schema(MIRROR_METADATA_SCHEMA_DQL).await {
                Ok(()) => return Ok(Self { client }),
                Err(error) if attempt < 3 => {
                    last = error.to_string();
                    tokio::time::sleep(std::time::Duration::from_millis(250 * attempt)).await;
                }
                Err(error) => return Err(unavailable(error)),
            }
        }
        Err(unavailable(format!("the DDL slice did not settle: {last}")))
    }

    fn row_key(key: &MetadataKey, seq: u64) -> String {
        format!("{}#{seq}", key.subject())
    }
}

#[async_trait]
impl MirrorMetadataLedger for DgraphMirrorMetadataLedger {
    async fn head(&self, key: &MetadataKey) -> Result<Head, LedgerError> {
        let query = format!(
            "{{\n  q(func: eq(hodei.mirrormeta.subject, {}), first: 1, \
             orderdesc: hodei.mirrormeta.seq) {{\n    hodei.mirrormeta.seq \
             hodei.mirrormeta.kind hodei.mirrormeta.date hodei.mirrormeta.digest \
             hodei.mirrormeta.at\n  }}\n}}",
            dql_string(&key.subject())
        );
        let response = self.client.execute_dql(&query).await.map_err(unavailable)?;
        require_no_errors(&response.json)?;
        let Some(row) = response
            .json
            .get("q")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
        else {
            return Ok(Head::default());
        };
        let corrupt = |what: &str| unavailable(format!("a ledger row is unreadable: {what}"));
        let seq = row
            .get("hodei.mirrormeta.seq")
            .and_then(Value::as_u64)
            .ok_or_else(|| corrupt("seq"))?;
        let accepted_at = row
            .get("hodei.mirrormeta.at")
            .and_then(Value::as_str)
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.with_timezone(&Utc));
        match row.get("hodei.mirrormeta.kind").and_then(Value::as_str) {
            Some("reset") => Ok(Head {
                seq,
                baseline: None,
                accepted_at,
            }),
            Some("accept") => {
                let date = row
                    .get("hodei.mirrormeta.date")
                    .and_then(Value::as_i64)
                    .and_then(|secs| Utc.timestamp_opt(secs, 0).single())
                    .ok_or_else(|| corrupt("date"))?;
                let digest = row
                    .get("hodei.mirrormeta.digest")
                    .and_then(Value::as_str)
                    .ok_or_else(|| corrupt("digest"))?
                    .to_string();
                Ok(Head {
                    seq,
                    baseline: Some(Baseline { date, digest }),
                    accepted_at,
                })
            }
            _ => Err(corrupt("kind")),
        }
    }

    async fn append(
        &self,
        key: &MetadataKey,
        seq: u64,
        step: &Step,
        at: DateTime<Utc>,
    ) -> Result<bool, LedgerError> {
        let row_key = Self::row_key(key, seq);
        let mut quads = format!(
            "_:row <dgraph.type> \"{ROW_TYPE}\" .\n\
             _:row <hodei.mirrormeta.key> {} .\n\
             _:row <hodei.mirrormeta.subject> {} .\n\
             _:row <hodei.mirrormeta.seq> \"{seq}\" .\n\
             _:row <hodei.mirrormeta.at> {} .\n",
            dql_string(&row_key),
            dql_string(&key.subject()),
            dql_string(&at.to_rfc3339()),
        );
        match step {
            Step::Accept(baseline) => quads.push_str(&format!(
                "_:row <hodei.mirrormeta.kind> \"accept\" .\n\
                 _:row <hodei.mirrormeta.date> \"{}\" .\n\
                 _:row <hodei.mirrormeta.digest> {} .\n",
                baseline.date.timestamp(),
                dql_string(&baseline.digest),
            )),
            Step::Reset { actor } => quads.push_str(&format!(
                "_:row <hodei.mirrormeta.kind> \"reset\" .\n\
                 _:row <hodei.mirrormeta.actor> {} .\n",
                dql_string(actor),
            )),
        }
        let transaction = GraphUpsertTransaction {
            query: format!(
                "{{\n  dupe as var(func: eq(hodei.mirrormeta.key, {}))\n  \
                 d(func: uid(dupe)) {{ uid }}\n}}",
                dql_string(&row_key)
            ),
            blocks: vec![GraphMutationBlock::set_if("eq(len(dupe), 0)", quads)],
            commit_now: true,
        };
        match self.client.upsert(transaction).await {
            Ok(response) => {
                require_no_errors(&response.json)?;
                let taken = response
                    .json
                    .get("d")
                    .and_then(Value::as_array)
                    .is_some_and(|rows| !rows.is_empty());
                Ok(!taken)
            }
            Err(hodei_graph_client::GraphClientError::UpsertConflict { .. }) => Ok(false),
            Err(error) => Err(unavailable(error)),
        }
    }
}
