//! The Cedar schema storage of ONE tenant, in the remote graph authority.
//!
//! This is the Dgraph port of the Surreal schema adapter: the same
//! [`SchemaStoragePort`] contract with the same observable semantics.
//!
//! - A schema is stored under its own version, so the version is the record
//!   identity. A schema saved without a version is stored under
//!   [`UNVERSIONED_SCHEMA_ID`] and reads back with that version.
//! - Saving is an idempotent replacement: saving the same version again
//!   overwrites the content (the service boots more than once per database).
//! - The latest schema is the most recently stored one.
//! - `save_schema` answers `schema:<version>` and `delete_schema` accepts
//!   both that spelling and the bare version.
//!
//! # Isolation
//!
//! A store is bound to ONE tenant. The node key is `tenant|version` and every
//! read filters on the tenant as well, so two tenants that store the same
//! version hold two nodes and never see each other's schema.
//!
//! # Fail-closed
//!
//! A server that does not answer is an error, never "no schema": a consumer
//! that reads an absent schema degrades policy validation to a syntax check.
//! A write that does not commit, or a record that is not there afterwards,
//! is an error as well.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hodei_graph_client::{
    GraphClient, GraphClientError, GraphMutationBlock, GraphMutationPort, GraphUpsertTransaction,
    dql_string,
};
use hodei_policies::build_schema::error::BuildSchemaError;
use hodei_policies::build_schema::ports::{SchemaStoragePort, StoredSchema};
use serde_json::Value;

/// The id under which a schema saved without a version is stored.
pub const UNVERSIONED_SCHEMA_ID: &str = "latest";

/// The type tag of the schema nodes in the graph (declared in the v9 schema).
pub const CEDAR_SCHEMA_TYPE: &str = "HodeiCedarSchema";

/// How many times a save may lose the write race before giving up.
const MAX_ATTEMPTS: usize = 8;

/// The Cedar schemas of ONE tenant, in Dgraph.
#[derive(Clone)]
pub struct DgraphSchemaStore {
    client: Arc<GraphClient>,
    tenant: String,
}

fn storage_error(error: impl ToString) -> BuildSchemaError {
    BuildSchemaError::SchemaStorageError(error.to_string())
}

impl DgraphSchemaStore {
    /// The schema store of a tenant, over a connected client.
    pub fn new(client: Arc<GraphClient>, tenant: impl Into<String>) -> Self {
        Self {
            client,
            tenant: tenant.into(),
        }
    }

    /// The identity key of one stored schema: the tenant is half of it.
    pub fn schema_key(tenant: &str, version: &str) -> String {
        format!("{tenant}|{version}")
    }

    /// Runs a read and returns the rows of the `q` root.
    async fn rows(&self, query: &str) -> Result<Vec<Value>, BuildSchemaError> {
        let response = self.client.execute_dql(query).await.map_err(storage_error)?;
        Ok(response
            .json
            .get("q")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// Decodes a row into the version and the content it must carry: a
    /// node without either is corrupt, never an absent schema.
    fn decode(row: &Value) -> Result<(String, String), BuildSchemaError> {
        let field = |name: &str| {
            row.get(name)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| storage_error(format!("a schema node has no {name}")))
        };
        Ok((field("hodei.cedar.version")?, field("hodei.cedar.content")?))
    }

    fn now_micros() -> i64 {
        chrono::Utc::now().timestamp_micros()
    }
}

#[async_trait]
impl SchemaStoragePort for DgraphSchemaStore {
    async fn save_schema(
        &self,
        schema_json: String,
        version: Option<String>,
    ) -> Result<String, BuildSchemaError> {
        let schema_id = version.unwrap_or_else(|| UNVERSIONED_SCHEMA_ID.to_string());
        let key = Self::schema_key(&self.tenant, &schema_id);
        let set_nquads = format!(
            "uid(v) <dgraph.type> \"{CEDAR_SCHEMA_TYPE}\" .\n\
             uid(v) <hodei.cedar.key> {} .\n\
             uid(v) <hodei.cedar.tenant> {} .\n\
             uid(v) <hodei.cedar.version> {} .\n\
             uid(v) <hodei.cedar.content> {} .\n\
             uid(v) <hodei.cedar.stored_at> \"{}\"^^<xs:int> .\n",
            dql_string(&key),
            dql_string(&self.tenant),
            dql_string(&schema_id),
            dql_string(&schema_json),
            Self::now_micros(),
        );
        // Create-or-replace, decided by the server: the key is `@upsert`,
        // so two writers of the same version meet there and the loser
        // retries on top of whatever committed.
        let transaction = GraphUpsertTransaction {
            query: format!("{{ v as var(func: eq(hodei.cedar.key, {})) }}", dql_string(&key)),
            blocks: vec![GraphMutationBlock {
                cond: None,
                set_nquads,
                delete_nquads: String::new(),
            }],
            commit_now: true,
        };
        let mut committed = false;
        for attempt in 0..MAX_ATTEMPTS {
            match self.client.upsert(transaction.clone()).await {
                Ok(_) => {
                    committed = true;
                    break;
                }
                Err(GraphClientError::UpsertConflict { .. }) if attempt + 1 < MAX_ATTEMPTS => {
                    tokio::time::sleep(Duration::from_millis(10u64 << attempt.min(6) as u32))
                        .await;
                }
                Err(error) => return Err(storage_error(error)),
            }
        }
        if !committed {
            return Err(storage_error(format!(
                "the schema write did not commit after {MAX_ATTEMPTS} attempts"
            )));
        }
        // The answer is what the store holds: a schema that is not there
        // after the write is an error.
        if self.get_schema_by_version(&schema_id).await?.is_none() {
            return Err(storage_error(format!(
                "the schema '{schema_id}' is not in the store after the write"
            )));
        }
        Ok(format!("schema:{schema_id}"))
    }

    async fn get_latest_schema(&self) -> Result<Option<String>, BuildSchemaError> {
        Ok(self
            .get_latest_schema_entry()
            .await?
            .map(|entry| entry.schema_string))
    }

    async fn get_latest_schema_entry(&self) -> Result<Option<StoredSchema>, BuildSchemaError> {
        let query = format!(
            "{{ q(func: eq(hodei.cedar.tenant, {}), orderdesc: hodei.cedar.stored_at, first: 1) \
             {{ hodei.cedar.version hodei.cedar.content }} }}",
            dql_string(&self.tenant),
        );
        let rows = self.rows(&query).await?;
        let Some(row) = rows.first() else {
            return Ok(None);
        };
        let (version, content) = Self::decode(row)?;
        Ok(Some(StoredSchema::new(
            content,
            Some(version.clone()),
            format!("schema:{version}"),
        )))
    }

    async fn get_schema_by_version(
        &self,
        version: &str,
    ) -> Result<Option<String>, BuildSchemaError> {
        let query = format!(
            "{{ q(func: eq(hodei.cedar.key, {})) @filter(eq(hodei.cedar.tenant, {})) \
             {{ hodei.cedar.version hodei.cedar.content }} }}",
            dql_string(&Self::schema_key(&self.tenant, version)),
            dql_string(&self.tenant),
        );
        let rows = self.rows(&query).await?;
        match rows.first() {
            None => Ok(None),
            Some(row) => Ok(Some(Self::decode(row)?.1)),
        }
    }

    async fn delete_schema(&self, schema_id: &str) -> Result<bool, BuildSchemaError> {
        // `save_schema` answers `schema:<version>` while the record is
        // stored under the bare version: both spellings delete it.
        let version = schema_id.strip_prefix("schema:").unwrap_or(schema_id);
        let key = Self::schema_key(&self.tenant, version);
        let mut delete = String::new();
        for predicate in [
            "hodei.cedar.key",
            "hodei.cedar.tenant",
            "hodei.cedar.version",
            "hodei.cedar.content",
            "hodei.cedar.stored_at",
            "dgraph.type",
        ] {
            delete.push_str(&format!("uid(d) <{predicate}> * .\n"));
        }
        let literal = dql_string(&key);
        let response = self
            .client
            .upsert(GraphUpsertTransaction {
                query: format!(
                    "{{ d as var(func: eq(hodei.cedar.key, {literal}))\n \
                     dstage(func: eq(hodei.cedar.key, {literal})) {{ uid }} }}"
                ),
                blocks: vec![GraphMutationBlock {
                    cond: Some("eq(len(d), 1)".to_owned()),
                    set_nquads: String::new(),
                    delete_nquads: delete,
                }],
                commit_now: true,
            })
            .await
            .map_err(storage_error)?;
        Ok(response
            .json
            .get("dstage")
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty()))
    }

    async fn list_schema_versions(&self) -> Result<Vec<String>, BuildSchemaError> {
        let query = format!(
            "{{ q(func: eq(hodei.cedar.tenant, {})) {{ hodei.cedar.version hodei.cedar.content }} }}",
            dql_string(&self.tenant),
        );
        let mut versions = self
            .rows(&query)
            .await?
            .iter()
            .map(|row| Self::decode(row).map(|(version, _)| version))
            .collect::<Result<Vec<_>, _>>()?;
        versions.sort();
        Ok(versions)
    }
}
