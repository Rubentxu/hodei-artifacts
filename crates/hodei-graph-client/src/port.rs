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
