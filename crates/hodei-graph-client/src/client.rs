//! The async client that talks to a real Dgraph Alpha.
//!
//! This is the **probe** half of the contract: everything here opens a
//! connection, performs a login or issues a request. It consumes a
//! [`GraphClientConfig`](crate::config::GraphClientConfig) that has already
//! been parsed and validated, loads the TLS material it names, and turns
//! every transport failure into a typed
//! [`GraphClientError`](crate::error::GraphClientError).
//!
//! Secrets hygiene, enforced structurally: the password lives only inside
//! the login request; the access token lives only in this struct and in
//! the gRPC metadata of subsequent requests; neither is ever formatted
//! into a log line, a `Debug` output or an error message. Log lines name
//! the endpoint, the phase and the username — nothing else.

use std::fmt;
use std::path::Path;
use std::time::Duration;

use prost::Message as _;
use tokio::time;
use tonic::metadata::MetadataValue;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};
use tracing::debug;

use crate::config::GraphClientConfig;
use crate::error::{GraphClientError, TlsMaterialInvalidReason};
use crate::generated as api;
use crate::generated::dgraph_client::DgraphClient;
use crate::port::{DqlResponse, GraphHealthPort, GraphQueryPort, GraphReadiness};

/// gRPC metadata key Dgraph v25.5.1 reads the access JWT from
/// (`x/x.go`: `md.Get("accessJwt")`). `X-Dgraph-AccessToken` is the
/// header of the *HTTP* surface (the CORS allow-list), a different
/// channel; on gRPC the metadata key is this one. It is pinned as a
/// constant so the wire detail is a deliberate edit, not folklore.
const ACCESS_TOKEN_HEADER: &str = "accessjwt";

/// The JSON sentinel of the readiness probe: a schema read is the
/// smallest query that exercises auth, indexes and the storage engine.
const READINESS_QUERY: &str = "schema {}";

/// A connected, authenticated client of one Dgraph Alpha.
pub struct GraphClient {
    /// The validated configuration the client was built from.
    config: GraphClientConfig,
    /// The established channel; every RPC reuses it.
    channel: Channel,
    /// The access token returned by the login, if a login happened.
    /// Never logged, never serialized.
    access_token: Option<String>,
}

impl fmt::Debug for GraphClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraphClient")
            .field("endpoint", &self.config.endpoint)
            .field("tls_enabled", &self.config.tls.enabled)
            .field("logged_in", &self.access_token.is_some())
            .finish_non_exhaustive()
    }
}

impl GraphClient {
    /// Connects to the configured endpoint.
    ///
    /// The TLS material named by the configuration is read here (it is a
    /// probe concern: the files must exist and parse *now*, not at parse
    /// time). The whole handshake is bounded by `connect_timeout_ms`.
    pub async fn connect(config: GraphClientConfig) -> Result<Self, GraphClientError> {
        // The config may have been built programmatically; validation is
        // idempotent and cheap, and never touches the network.
        config.validate()?;

        let uri = if config.tls.enabled {
            format!("https://{}", config.endpoint)
        } else {
            format!("http://{}", config.endpoint)
        };
        let mut endpoint = Endpoint::from_shared(uri.clone())
            .map_err(|source| GraphClientError::InvalidUri {
                endpoint: config.endpoint.clone(),
                source,
            })?
            .connect_timeout(Duration::from_millis(config.connect_timeout_ms))
            .timeout(Duration::from_millis(config.request_timeout_ms));

        if config.tls.enabled {
            // Validation guarantees the CA path is present.
            let ca_path = config
                .tls
                .ca_cert_path
                .as_ref()
                .expect("validation requires the CA path when TLS is enabled");
            let ca_pem = read_pem(ca_path)?;
            let mut tls = ClientTlsConfig::new()
                .ca_certificate(Certificate::from_pem(ca_pem))
                .domain_name(
                    config
                        .tls
                        .server_name
                        .clone()
                        .unwrap_or_else(|| config.host().to_owned()),
                );
            if let (Some(cert_path), Some(key_path)) =
                (&config.tls.client_cert_path, &config.tls.client_key_path)
            {
                let cert_pem = read_pem(cert_path)?;
                let key_pem = read_pem(key_path)?;
                tls = tls.identity(Identity::from_pem(cert_pem, key_pem));
            }
            // Building the TLS stack validates the material; a rejection
            // here is a connect failure of this endpoint.
            endpoint = endpoint
                .tls_config(tls)
                .map_err(|source| GraphClientError::Connect {
                    endpoint: config.endpoint.clone(),
                    source,
                })?;
        }

        let channel = time::timeout(
            Duration::from_millis(config.connect_timeout_ms),
            endpoint.connect(),
        )
        .await
        .map_err(|source| GraphClientError::ConnectTimeout {
            endpoint: config.endpoint.clone(),
            ms: config.connect_timeout_ms,
            source,
        })?
        .map_err(|source| GraphClientError::Connect {
            endpoint: config.endpoint.clone(),
            source,
        })?;

        debug!(endpoint = %config.endpoint, tls = config.tls.enabled, "connected to the Dgraph Alpha");
        Ok(Self {
            config,
            channel,
            access_token: None,
        })
    }

    /// Performs the ACL login and stores the access token for subsequent
    /// requests. Without configured credentials this is a no-op: an ACL
    /// server will then reject the authenticated calls explicitly.
    pub async fn login(&mut self) -> Result<(), GraphClientError> {
        let Some(credentials) = self.config.credentials.clone() else {
            debug!(endpoint = %self.config.endpoint, "no credentials configured: skipping login");
            return Ok(());
        };
        self.perform_login(&credentials).await
    }

    /// The one place a login happens; takes the credentials so both the
    /// explicit `login` and the readiness probe can use it.
    async fn perform_login(
        &mut self,
        credentials: &crate::config::GraphLoginCredentials,
    ) -> Result<(), GraphClientError> {
        let userid = credentials.userid().to_owned();
        let request = api::LoginRequest {
            userid: userid.clone(),
            password: credentials.password().to_owned(),
            refresh_token: String::new(),
            namespace: self.config.namespace,
        };
        let mut client = DgraphClient::new(self.channel.clone());
        let response =
            client
                .login(request)
                .await
                .map_err(|status| GraphClientError::LoginRejected {
                    userid,
                    detail: status.message().to_owned(),
                })?;
        // Dgraph v25 packs the login answer as a protobuf-marshaled
        // `api.Jwt` inside the response's `json` bytes (not JSON): the
        // wire format is the server's, and this is the only place the
        // client decodes it.
        let inner = response.into_inner();
        let jwt = api::Jwt::decode(inner.json.as_ref())
            .map_err(|_| GraphClientError::LoginWithoutToken)?;
        let token = jwt.access_jwt;
        if token.is_empty() {
            return Err(GraphClientError::LoginWithoutToken);
        }
        self.access_token = Some(token);
        debug!(userid = %credentials.userid(), endpoint = %self.config.endpoint, "ACL login accepted");
        Ok(())
    }

    /// Attaches the access token, if there is one, to a gRPC request.
    fn authenticated<T>(token: Option<&str>, mut request: tonic::Request<T>) -> tonic::Request<T> {
        if let Some(token) = token
            && let Ok(header) = MetadataValue::try_from(token)
        {
            request.metadata_mut().insert(ACCESS_TOKEN_HEADER, header);
        }
        request
    }

    /// Runs one DQL query and returns its JSON answer.
    pub async fn execute_dql(&self, query: &str) -> Result<DqlResponse, GraphClientError> {
        let request = api::Request {
            query: query.to_owned(),
            read_only: true,
            ..Default::default()
        };
        let mut client = DgraphClient::new(self.channel.clone());
        let response = client
            .query(Self::authenticated(
                self.access_token.as_deref(),
                tonic::Request::new(request),
            ))
            .await
            .map_err(|status| GraphClientError::Request {
                phase: "query",
                source: status,
            })?;
        let inner = response.into_inner();
        let json = serde_json::from_slice::<serde_json::Value>(&inner.json).map_err(|error| {
            GraphClientError::Request {
                phase: "query-json",
                source: tonic::Status::internal(error.to_string()),
            }
        })?;
        Ok(DqlResponse { json })
    }

    /// Reports the version tag of the server.
    pub async fn check_version(&self) -> Result<String, GraphClientError> {
        let mut client = DgraphClient::new(self.channel.clone());
        let response = client
            .check_version(Self::authenticated(
                self.access_token.as_deref(),
                tonic::Request::new(api::Check {}),
            ))
            .await
            .map_err(|status| GraphClientError::Request {
                phase: "check-version",
                source: status,
            })?;
        Ok(response.into_inner().tag)
    }

    /// Whether a login has been performed in this client.
    pub fn is_logged_in(&self) -> bool {
        self.access_token.is_some()
    }
}

/// Reads one TLS material file and requires it to look like PEM.
///
/// A file where a PEM document belongs that carries no `-----BEGIN`
/// marker is almost always a misconfiguration; naming it beats an opaque
/// TLS handshake failure later.
fn read_pem(path: &Path) -> Result<Vec<u8>, GraphClientError> {
    let bytes = std::fs::read(path).map_err(|source| GraphClientError::TlsMaterialIo {
        path: path.to_path_buf(),
        source,
    })?;
    let looks_like_pem = std::str::from_utf8(&bytes)
        .is_ok_and(|text| text.lines().any(|line| line.starts_with("-----BEGIN ")));
    if !looks_like_pem {
        return Err(GraphClientError::TlsMaterialInvalid {
            source: TlsMaterialInvalidReason::NotPemText {
                path: path.to_path_buf(),
            },
        });
    }
    Ok(bytes)
}

#[async_trait::async_trait]
impl GraphHealthPort for GraphClient {
    async fn ready(&self) -> Result<GraphReadiness, GraphClientError> {
        // The readiness probe needs a login, which mutates the client; the
        // port takes `&self` so consumers can probe shared instances. A
        // probe clone performs its own login and its own round-trip.
        let mut probe = Self {
            config: self.config.clone(),
            channel: self.channel.clone(),
            access_token: None,
        };
        let login_performed = match probe.config.credentials.clone() {
            Some(credentials) => {
                probe.perform_login(&credentials).await?;
                true
            }
            None => false,
        };

        // The version check is best effort: the readiness contract is the
        // authenticated DQL round-trip, not the version banner.
        let server_version = probe.check_version().await.ok();
        let response = probe.execute_dql(READINESS_QUERY).await?;
        let schema_roundtrip = response.json.get("schema").is_some();
        Ok(GraphReadiness {
            server_version,
            authenticated_query_served: schema_roundtrip,
            login_performed,
        })
    }
}

#[async_trait::async_trait]
impl GraphQueryPort for GraphClient {
    async fn execute_dql(&self, query: &str) -> Result<DqlResponse, GraphClientError> {
        GraphClient::execute_dql(self, query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::GraphTlsConfig;
    use std::path::PathBuf;

    fn plaintext_config(endpoint: &str, connect_timeout_ms: u64) -> GraphClientConfig {
        GraphClientConfig {
            endpoint: endpoint.to_owned(),
            tls: GraphTlsConfig {
                enabled: false,
                ca_cert_path: None,
                client_cert_path: None,
                client_key_path: None,
                server_name: None,
            },
            credentials: None,
            connect_timeout_ms,
            request_timeout_ms: connect_timeout_ms,
            namespace: 0,
        }
    }

    #[tokio::test]
    async fn connecting_to_a_closed_port_fails_explicitly() {
        // A port nothing listens on: the failure must be a typed connect
        // error, never a panic and never a silent success.
        let config = plaintext_config("127.0.0.1:1", 500);
        let error = GraphClient::connect(config)
            .await
            .expect_err("closed port must fail");
        assert!(
            matches!(
                error,
                GraphClientError::Connect { .. } | GraphClientError::ConnectTimeout { .. }
            ),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn connecting_to_a_dead_endpoint_fails_with_a_typed_error() {
        // RFC 5737 documentation address: nothing answers there. Whether
        // the host refuses, the route is unreachable or the connect
        // timeout fires, the failure must be one of the typed connect
        // errors — the explicit half of "no silent fallback".
        let config = plaintext_config("192.0.2.1:9080", 400);
        let error = GraphClient::connect(config)
            .await
            .expect_err("dead endpoint must fail");
        assert!(
            matches!(
                error,
                GraphClientError::Connect { .. } | GraphClientError::ConnectTimeout { .. }
            ),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn tls_material_that_does_not_exist_is_a_typed_error() {
        // TLS enabled against a CA that does not exist: the missing file
        // must be reported, not turned into a generic connect failure.
        let mut config = plaintext_config("127.0.0.1:1", 500);
        config.tls.enabled = true;
        config.tls.ca_cert_path = Some(PathBuf::from("/nonexistent/hodei-test-ca.crt"));
        let error = GraphClient::connect(config)
            .await
            .expect_err("missing CA must fail");
        assert!(
            matches!(error, GraphClientError::TlsMaterialIo { .. }),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn tls_material_that_is_not_pem_text_is_a_typed_error() {
        let mut config = plaintext_config("127.0.0.1:1", 500);
        config.tls.enabled = true;
        let ca = std::env::temp_dir().join("hodei-graph-client-invalid-ca.crt");
        std::fs::write(&ca, b"\x00\x01\x02definitely not a pem document")
            .expect("write the fake CA");
        config.tls.ca_cert_path = Some(ca);
        let error = GraphClient::connect(config)
            .await
            .expect_err("binary CA must fail");
        assert!(
            matches!(
                error,
                GraphClientError::TlsMaterialInvalid {
                    source: TlsMaterialInvalidReason::NotPemText { .. },
                }
            ),
            "unexpected error: {error}"
        );
    }
}
