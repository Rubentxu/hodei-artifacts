//! Minimal async client for Dgraph v25, driven by explicit configuration.
//!
//! What this crate is:
//!
//! - a gRPC client (tonic, protobuf versions pinned) for the official
//!   Dgraph client API of v25, vendored as `protos/api.proto`;
//! - TLS with an explicit CA (no system-store fallback), optional mutual
//!   TLS and an optional server-name override;
//! - the ACL login (JWT) with `LoginRequest`, tokens sent in the v25
//!   `x-dgraph-accesstoken` header;
//! - a readiness probe (`ready`) that performs the whole chain against a
//!   real server: connect, login, version check and one DQL round-trip;
//! - one DQL query port; nothing else. No ORM, no mutation surface, no
//!   schema management.
//!
//! What this crate is not:
//!
//! - a universal database abstraction. The kernel and the bounded
//!   contexts depend on the traits in [`port`], which mention no SDK
//!   type; this client is only one adapter behind them.
//!
//! The configuration/config-probe split is the core of the contract:
//!
//! - [`config`] is pure: parsing, structural validation, redaction of
//!   credentials. It never opens a socket and never reads a file, so a
//!   deployment can be validated without a server.
//! - [`client`] is the probe: it reads the TLS material, dials the
//!   endpoint, performs the login and issues the requests. Every failure
//!   of every step is a typed variant of [`error::GraphClientError`].
//!
//! There is no silent fallback anywhere: no endpoint does not mean
//! `localhost`, TLS without a CA does not mean system roots, incomplete
//! credentials do not mean anonymous. Everything unconfigured is a typed
//! error, and everything configured is explicit in the document.

#![warn(missing_docs)]
// The typed error carries tonic's `Status` and transport error by value
// (~200 bytes). These results are constructed only on failure paths and
// never stored hot, so boxing would complicate every match on the port's
// error for no measurable gain.
#![allow(clippy::result_large_err)]

pub mod client;
pub mod config;
pub mod error;
pub mod generated;
pub mod identity;
pub mod port;
pub mod schema;

pub use client::GraphClient;
pub use config::{GraphClientConfig, GraphLoginCredentials, GraphTlsConfig};
pub use error::{GraphClientError, GraphConfigError, GraphConfigLoadError};
pub use identity::{
    CanonicalHrn, IDENTITY_TYPE, IdentityError, IdentityRecord, IdentityStore, KEY_SEPARATOR,
    Tenant, identity_digest, identity_key,
};
pub use port::{
    DqlResponse, GraphHealthPort, GraphMutationBlock, GraphMutationPort, GraphQueryPort,
    GraphReadiness, GraphUpsertTransaction,
};
pub use schema::{
    BootDecision, HodeiSchema, PredicateSpec, SCHEMA_VERSION, SchemaBootError, SchemaBootReport,
    decide_boot, ensure_ready, hodei_schema, verify_predicates, write_schema_meta,
};
