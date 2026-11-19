//! The ports the rest of the system may depend on.
//!
//! The kernel and the bounded contexts of this workspace never depend on a
//! storage SDK: they depend on traits. These are the graph traits. Nothing
//! in their signatures mentions tonic, protobuf or the Dgraph wire
//! protocol, so a caller that depends on them stays decoupled from the SDK
//! (AC#3 of the graph-first contract), and this client is free to change
//! its transport without touching any consumer.
//!
//! There is no ORM here either: a DQL query is a string the caller owns,
//! and the answer comes back as JSON.

use async_trait::async_trait;

use crate::error::GraphClientError;

/// The answer to one DQL query, already decoded from JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct DqlResponse {
    /// The JSON document the server produced for the query.
    pub json: serde_json::Value,
}

/// Whether the graph authority is ready to serve this deployment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphReadiness {
    /// The version tag the server reports, when it reports one.
    pub server_version: Option<String>,
    /// Whether an authenticated DQL round-trip succeeded.
    pub authenticated_query_served: bool,
    /// Whether the probe performed an ACL login (i.e. credentials were
    /// configured and accepted). `false` with a success overall means the
    /// server let the client through without a login.
    pub login_performed: bool,
}

/// The health/readiness port of the graph authority.
#[async_trait]
pub trait GraphHealthPort: Send + Sync {
    /// Probes the server end to end: connect, (optional) login, version
    /// check and one authenticated DQL round-trip. Any step that fails
    /// fails the probe with the typed cause.
    async fn ready(&self) -> Result<GraphReadiness, GraphClientError>;
}

/// The query port of the graph authority.
#[async_trait]
pub trait GraphQueryPort: Send + Sync {
    /// Runs one DQL query and returns its JSON answer. No mutation is
    /// performed: queries are read-only by construction of this port.
    async fn execute_dql(&self, query: &str) -> Result<DqlResponse, GraphClientError>;
}

/// One mutation block inside an upsert transaction: the N-Quads it sets
/// or deletes, optionally guarded by a condition over the query's result
/// variables (`@if(eq(var, value))`).
///
/// This is a plain DTO on purpose: the port mentions no tonic or protobuf
/// type, so callers stay decoupled from the Dgraph SDK.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphMutationBlock {
    /// The upsert condition that gates this block, in DQL condition
    /// syntax without the `@if(...)` wrapper (e.g. `eq(len(u), 0)`).
    /// `None` means the block always applies.
    pub cond: Option<String>,
    /// N-Quads to set. Empty means nothing is set.
    pub set_nquads: String,
    /// N-Quads to delete. Empty means nothing is deleted.
    pub delete_nquads: String,
}

impl GraphMutationBlock {
    /// An unconditional block that only sets N-Quads.
    pub fn set(set_nquads: impl Into<String>) -> Self {
        Self {
            cond: None,
            set_nquads: set_nquads.into(),
            delete_nquads: String::new(),
        }
    }

    /// A block gated by an upsert condition over the query variables.
    pub fn set_if(cond: impl Into<String>, set_nquads: impl Into<String>) -> Self {
        Self {
            cond: Some(cond.into()),
            set_nquads: set_nquads.into(),
            delete_nquads: String::new(),
        }
    }
}

/// One upsert transaction: a DQL query that binds result variables,
/// followed by mutation blocks that may reference those variables, all
/// committed atomically (either every block commits or none does).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GraphUpsertTransaction {
    /// The DQL query block whose variables the mutations reference.
    pub query: String,
    /// The mutation blocks, applied in order under their conditions.
    pub blocks: Vec<GraphMutationBlock>,
    /// Whether the server must commit (or abort) immediately with the
    /// response. Uniqueness upserts set this: the answer decides whether
    /// the caller won or lost the write race.
    pub commit_now: bool,
}

/// The write port of the graph authority: atomic upsert transactions and
/// schema alterations. Like the query port, it mentions no SDK type.
#[async_trait]
pub trait GraphMutationPort: Send + Sync {
    /// Runs one upsert transaction. The JSON answer carries the results
    /// of the transaction's query block (including the bound variables),
    /// so the caller can tell what the server actually did.
    ///
    /// Losing a write race on a predicate marked `@upsert` surfaces as
    /// [`GraphClientError::UpsertConflict`]: the server enforced the
    /// unique index and aborted this transaction.
    async fn upsert(
        &self,
        transaction: GraphUpsertTransaction,
    ) -> Result<DqlResponse, GraphClientError>;

    /// Alters the schema of the graph (new predicates, types, indexes).
    /// Alterations are additive and idempotent when the schema text is
    /// idempotent: re-sending an identical schema is accepted as a
    /// no-op by the server.
    async fn alter_schema(&self, schema_dql: &str) -> Result<(), GraphClientError>;
}
