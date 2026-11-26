//! The trusted publishers registry over the graph authority (Dgraph).
//!
//! One node per record, keyed by `account/repository/id` with an `@upsert`
//! exact index so concurrent writers of the same record converge on ONE node
//! (the server aborts the loser). The record travels as one JSON document:
//! the registry decides nothing, it stores what an administrator authorized.

use std::sync::Arc;

use async_trait::async_trait;
use hodei_graph_client::port::{GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction};
use hodei_graph_client::schema::dql_string;
use hodei_graph_client::{GraphClient, GraphClientError};
use serde_json::Value;

use super::{PublisherRegistry, RegistryError, TrustedPublisher};

/// The node type of one trusted publisher.
pub const PUBLISHER_TYPE: &str = "HodeiTrustedPublisher";

/// The DDL slice of this adapter: additive, applied idempotently.
pub const PUBLISHER_SCHEMA_DQL: &str = "
hodei.publisher.key: string @index(exact) @upsert .
hodei.publisher.scope: string @index(exact) .
hodei.publisher.doc: string .
type HodeiTrustedPublisher {
  hodei.publisher.key
  hodei.publisher.scope
  hodei.publisher.doc
}
";

/// The most records one repository can hold: the exchange only reads a few.
const MAX_LIST: usize = 100;

const MAX_ATTEMPTS: usize = 12;

/// The registry over Dgraph.
pub struct DgraphPublisherRegistry {
    client: Arc<GraphClient>,
}

impl DgraphPublisherRegistry {
    /// Open the registry, applying its DDL slice.
    pub async fn open(client: Arc<GraphClient>) -> Result<Self, RegistryError> {
        let mut last = String::new();
        for attempt in 1..=MAX_ATTEMPTS {
            match client.alter_schema(PUBLISHER_SCHEMA_DQL).await {
                Ok(()) => return Ok(Self { client }),
                Err(error) => {
                    last = error.to_string();
                    tokio::time::sleep(std::time::Duration::from_millis(100 * attempt as u64))
                        .await;
                }
            }
        }
        Err(RegistryError(format!("the schema did not settle: {last}")))
    }

    async fn write(
        &self,
        key: &str,
        set: String,
        delete: String,
        gate: Option<&str>,
    ) -> Result<Value, RegistryError> {
        let query = format!(
            "{{ q(func: eq(hodei.publisher.key, {key})) {{ u as uid }} }}",
            key = dql_string(key)
        );
        let blocks = vec![GraphMutationBlock {
            cond: gate.map(str::to_string),
            set_nquads: set,
            delete_nquads: delete,
        }];
        let mut last = String::new();
        for round in 1..=MAX_ATTEMPTS {
            match self
                .client
                .upsert(GraphUpsertTransaction {
                    query: query.clone(),
                    blocks: blocks.clone(),
                    commit_now: true,
                })
                .await
            {
                Ok(response) => return Ok(response.json),
                Err(GraphClientError::UpsertConflict { detail }) => {
                    last = detail;
                    // Backoff with jitter: racing writers must not retry in lockstep.
                    let jitter: u64 = rand::random::<u8>() as u64 % 16;
                    tokio::time::sleep(std::time::Duration::from_millis(
                        (10 * round as u64).min(120) + jitter,
                    ))
                    .await;
                }
                Err(error) => return Err(RegistryError(error.to_string())),
            }
        }
        Err(RegistryError(format!(
            "write conflict did not settle: {last}"
        )))
    }
}

fn key_of(account: &str, repository: &str, id: &str) -> String {
    format!("{account}/{repository}/{id}")
}

#[async_trait]
impl PublisherRegistry for DgraphPublisherRegistry {
    async fn put(&self, p: &TrustedPublisher) -> Result<(), RegistryError> {
        let doc = serde_json::to_string(p).map_err(|e| RegistryError(e.to_string()))?;
        let key = key_of(&p.account, &p.repository, &p.id);
        let set = format!(
            "uid(u) <dgraph.type> \"{PUBLISHER_TYPE}\" .\n\
             uid(u) <hodei.publisher.key> {key} .\n\
             uid(u) <hodei.publisher.scope> {scope} .\n\
             uid(u) <hodei.publisher.doc> {doc} .\n",
            key = dql_string(&key),
            scope = dql_string(&format!("{}/{}", p.account, p.repository)),
            doc = dql_string(&doc),
        );
        self.write(&key, set, String::new(), None).await.map(|_| ())
    }

    async fn delete(
        &self,
        account: &str,
        repository: &str,
        id: &str,
    ) -> Result<bool, RegistryError> {
        let key = key_of(account, repository, id);
        let existed = !self
            .read(&format!("eq(hodei.publisher.key, {})", dql_string(&key)))
            .await?
            .is_empty();
        self.write(
            &key,
            String::new(),
            "uid(u) * * .\n".to_string(),
            Some("gt(len(u), 0)"),
        )
        .await?;
        Ok(existed)
    }

    async fn list(
        &self,
        account: &str,
        repository: &str,
    ) -> Result<Vec<TrustedPublisher>, RegistryError> {
        let scope = format!("{account}/{repository}");
        self.read(&format!(
            "eq(hodei.publisher.scope, {})",
            dql_string(&scope)
        ))
        .await
    }
}

impl DgraphPublisherRegistry {
    async fn read(&self, func: &str) -> Result<Vec<TrustedPublisher>, RegistryError> {
        // One more than the maximum, ordered: a listing never silently hides
        // records, it fails when there are more than it can return.
        let query = format!(
            "{{ q(func: {func}, orderasc: hodei.publisher.key, first: {}) {{ hodei.publisher.doc }} }}",
            MAX_LIST + 1
        );
        let response = self
            .client
            .execute_dql(&query)
            .await
            .map_err(|e| RegistryError(e.to_string()))?;
        if response
            .json
            .get("errors")
            .and_then(Value::as_array)
            .is_some_and(|errors| !errors.is_empty())
        {
            return Err(RegistryError("the graph refused the query".into()));
        }
        let nodes = response
            .json
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if nodes.len() > MAX_LIST {
            return Err(RegistryError(
                "more records than the listing can return".into(),
            ));
        }
        nodes
            .iter()
            .map(|node| {
                let text = node
                    .get("hodei.publisher.doc")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RegistryError("a record without document".into()))?;
                serde_json::from_str(text).map_err(|_| RegistryError("a corrupt record".into()))
            })
            .collect()
    }
}
