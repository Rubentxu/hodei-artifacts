//! Typed failures of the graph client.
//!
//! Every way this client can fail is a variant of [`GraphClientError`] (or
//! of the configuration error it wraps). There is no catch-all string and
//! no silent degradation: a caller that matches on the error knows whether
//! the deployment is misconfigured, the server is unreachable, the
//! credentials were rejected or the server answered with a protocol error.
//!
//! Secrets never take part in any message: passwords are redacted in every
//! `Debug` and `Display` implementation, and server details quoted in
//! [`GraphClientError::LoginRejected`] come from the server's status text,
//! which echoes the rejection reason, never the credentials themselves.

use std::path::PathBuf;

/// How the pure configuration is wrong.
///
/// Detection of these errors never touches the network or the filesystem:
/// a caller can validate a deployment's configuration at startup without
/// any server, which is what makes the "probe" separable from the "config".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GraphConfigError {
    /// No endpoint was given at all.
    #[error("the Dgraph endpoint is missing: graph connections are never guessed")]
    MissingEndpoint,
    /// The endpoint carries a scheme (`https://...`); TLS is decided by the
    /// explicit `tls.enabled` flag, not by the text of the endpoint.
    #[error(
        "the Dgraph endpoint '{endpoint}' carries the scheme '{scheme}'; remove it: TLS is chosen with tls.enabled, not in the endpoint"
    )]
    EndpointWithScheme {
        /// The scheme found in the endpoint text.
        scheme: String,
        /// The endpoint as written in the configuration.
        endpoint: String,
    },
    /// The endpoint names no port; Dgraph's gRPC port is not assumed.
    #[error(
        "the Dgraph endpoint '{endpoint}' has no port; the gRPC port of the Alpha is explicit configuration, never a default"
    )]
    EndpointWithoutPort {
        /// The endpoint as written in the configuration.
        endpoint: String,
    },
    /// The port text is not a valid TCP port.
    #[error(
        "the endpoint '{endpoint}' has an invalid port '{port}': it must be a TCP port between 1 and 65535"
    )]
    InvalidPort {
        /// The endpoint as written in the configuration.
        endpoint: String,
        /// The port text that could not be parsed.
        port: String,
    },
    /// TLS is enabled but the root of trust is not named.
    #[error(
        "tls.enabled = true but no ca_cert_path is set: the Dgraph CA is required; there is no silent fallback to the system trust store"
    )]
    TlsEnabledWithoutCaCertificate,
    /// Only one half of an optional client identity was given.
    #[error(
        "incomplete client identity: has_cert = {has_cert}, has_key = {has_key}; both tls.client_cert_path and tls.client_key_path are needed together"
    )]
    ClientIdentityIncomplete {
        /// Whether the client certificate path is present.
        has_cert: bool,
        /// Whether the client key path is present.
        has_key: bool,
    },
    /// A timeout of zero would turn every call into an immediate failure.
    #[error("the timeout '{field}' is zero: give it a real bound in milliseconds")]
    ZeroTimeout {
        /// The name of the timeout field that is zero.
        field: &'static str,
    },
    /// A login was attempted with an incomplete pair of credentials.
    #[error(
        "incomplete credentials: the field '{field}' is missing; a login is either complete or absent"
    )]
    MissingCredentialsField {
        /// The name of the credentials field that is missing or empty.
        field: &'static str,
    },
    /// The credentials block names a user but the password is empty.
    #[error("the password of the credentials is empty: an empty password is not a credential")]
    EmptyPassword,
}

/// How loading the configuration from text failed.
#[derive(Debug, thiserror::Error)]
pub enum GraphConfigLoadError {
    /// The configuration text is not valid TOML.
    #[error("the graph client configuration is not valid TOML: {source}")]
    Parse {
        /// The TOML parse error underneath.
        #[from]
        source: toml::de::Error,
    },
    /// The parsed configuration does not satisfy the validation rules.
    #[error("the graph client configuration is invalid: {0}")]
    Invalid(#[from] GraphConfigError),
}

/// Every explicit failure of a graph operation.
#[derive(Debug, thiserror::Error)]
pub enum GraphClientError {
    /// The configuration itself is wrong; nothing was dialed.
    #[error("graph client configuration is invalid: {0}")]
    Configuration(#[from] GraphConfigError),
    /// The configuration text could not even be parsed or validated.
    #[error(transparent)]
    ConfigurationLoad(#[from] GraphConfigLoadError),
    /// A TLS material file named by the configuration could not be read.
    #[error("the TLS material at '{path}' could not be read: {source}")]
    TlsMaterialIo {
        /// The path of the file that could not be read.
        path: PathBuf,
        /// The IO error underneath.
        source: std::io::Error,
    },
    /// The TLS material exists but is not a parseable PEM document.
    #[error("the TLS material given to the graph client is not valid PEM: {source}")]
    TlsMaterialInvalid {
        /// Why the material was rejected.
        source: TlsMaterialInvalidReason,
    },
    /// The endpoint text is not a valid URI at all.
    #[error("the Dgraph endpoint '{endpoint}' is not a valid connection target: {source}")]
    InvalidUri {
        /// The endpoint as written in the configuration.
        endpoint: String,
        /// The URI parse error underneath.
        source: tonic::transport::Error,
    },
    /// The TCP/TLS/HTTP2 handshake did not finish inside the connect timeout.
    #[error("connecting to the Dgraph endpoint '{endpoint}' exceeded the {ms} ms connect timeout")]
    ConnectTimeout {
        /// The endpoint that was being dialed.
        endpoint: String,
        /// The connect timeout that fired, in milliseconds.
        ms: u64,
        /// The elapsed error underneath.
        source: tokio::time::error::Elapsed,
    },
    /// The connection could not be established.
    #[error("connecting to the Dgraph endpoint '{endpoint}' failed: {source}")]
    Connect {
        /// The endpoint that was being dialed.
        endpoint: String,
        /// The transport error underneath (TLS rejection, refused TCP, ...).
        source: tonic::transport::Error,
    },
    /// The server rejected the login for the named user.
    #[error("the Dgraph server rejected the login for user '{userid}': {detail}")]
    LoginRejected {
        /// The user the server rejected (the password is never quoted).
        userid: String,
        /// The server's own rejection detail.
        detail: String,
    },
    /// The login succeeded but the server returned no access token.
    #[error("the Dgraph server accepted the login but returned no access token")]
    LoginWithoutToken,
    /// A gRPC request (DQL, health check) was answered with a protocol error.
    #[error("the Dgraph server rejected the request '{phase}' with a protocol error: {source}")]
    Request {
        /// The logical phase that failed (`query`, `check-version`, ...).
        phase: &'static str,
        /// The gRPC status the server answered with.
        source: tonic::Status,
    },
    /// A gRPC request did not finish inside the request timeout.
    #[error("the request '{phase}' exceeded the {ms} ms request timeout")]
    RequestTimeout {
        /// The logical phase whose deadline fired.
        phase: &'static str,
        /// The request timeout that fired, in milliseconds.
        ms: u64,
        /// The elapsed error underneath.
        source: tokio::time::error::Elapsed,
    },
    /// An indexed upsert lost the write race on a unique predicate.
    ///
    /// With `@upsert` on a predicate, Dgraph v25 rejects any transaction
    /// that would commit a second edge over the same index key
    /// (`ErrIndexingConflict`). This is not a failure of the operation:
    /// it is the server enforcing uniqueness, and the correct answer is
    /// to revalidate and retry, which is why callers can distinguish it.
    #[error("the upsert conflicted with a concurrent writer on an indexed predicate: {detail}")]
    UpsertConflict {
        /// The server's own conflict detail (no secrets: DQL text only).
        detail: String,
    },
}

/// Why a PEM document was rejected.
#[derive(Debug, thiserror::Error)]
pub enum TlsMaterialInvalidReason {
    /// The named file exists but is not text, so it cannot be a PEM
    /// document (the CA, a certificate or a key).
    #[error("the TLS material at '{path}' is not text and therefore not a PEM document")]
    NotPemText {
        /// The path of the file that is not PEM text.
        path: PathBuf,
    },
}
