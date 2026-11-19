//! Pure configuration of the graph client.
//!
//! This module is the **config** half of the contract: parsing, structural
//! validation and redaction. It never opens a socket and never reads a
//! file, so every rule below can be tested without a server. The **probe**
//! half (actually dialing, logging in, asking for readiness) lives in
//! [`crate::client`].
//!
//! The one rule that matters most: **there are no implicit fallbacks.**
//! A deployment without an endpoint does not silently try `localhost`;
//! TLS without a CA does not silently trust the system store; a login
//! without a complete pair of credentials does not silently go anonymous.
//! Everything that is not configured is a typed error the caller sees.

use std::fmt;
use std::path::PathBuf;

use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use crate::error::GraphConfigError;

/// Default connect timeout in milliseconds: five seconds is enough for a
/// local cluster and keeps a dead endpoint from hanging a boot.
pub const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 5_000;

/// Default per-request timeout in milliseconds: ten seconds bounds every
/// single RPC (login, health check, DQL query).
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 10_000;

/// Everything needed to reach one Dgraph Alpha over gRPC.
///
/// Serialized from TOML (or equivalent sources). Every field that could
/// hide a fallback is required in `validate` instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphClientConfig {
    /// The gRPC endpoint of the Alpha, as `host:port`. No scheme: whether
    /// the connection uses TLS is the `tls.enabled` flag, not the text of
    /// the endpoint.
    pub endpoint: String,
    /// TLS configuration. Present even for plaintext deployments so the
    /// choice is written down instead of implied.
    pub tls: GraphTlsConfig,
    /// ACL credentials. `None` means the client will *not* attempt a login
    /// and every authenticated call fails explicitly if the server demands
    /// one; `Some` means the pair must be complete.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<GraphLoginCredentials>,
    /// Bound for the whole TCP/TLS/HTTP2 handshake. Default:
    /// [`DEFAULT_CONNECT_TIMEOUT_MS`].
    #[serde(default = "default_connect_timeout_ms")]
    pub connect_timeout_ms: u64,
    /// Bound for every single RPC. Default:
    /// [`DEFAULT_REQUEST_TIMEOUT_MS`].
    #[serde(default = "default_request_timeout_ms")]
    pub request_timeout_ms: u64,
    /// ACL namespace of the login. Default `0`, the guardian namespace of
    /// a standard Dgraph cluster; this default is documented protocol
    /// behavior, not a guess about *where* the server is.
    #[serde(default)]
    pub namespace: u64,
}

/// The TLS choice of the deployment, written down explicitly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphTlsConfig {
    /// Whether the gRPC connection uses TLS. `false` is a deliberate,
    /// visible choice (never the result of missing configuration).
    pub enabled: bool,
    /// Path to the PEM root CA that signed the server certificate. Required
    /// whenever `enabled` is true: a Dgraph cluster issues its own CA, and
    /// the system trust store would never contain it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_cert_path: Option<PathBuf>,
    /// Optional path to the client certificate for mutual TLS. If set,
    /// `client_key_path` must be set too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_cert_path: Option<PathBuf>,
    /// Optional path to the client key for mutual TLS. If set,
    /// `client_cert_path` must be set too.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_key_path: Option<PathBuf>,
    /// Optional override of the DNS name used to verify the server
    /// certificate, for deployments where the Alpha is reached by address
    /// but its certificate is issued for a node name (the standard outcome
    /// of `dgraph cert --nodes ...`). Absent means "verify against the
    /// host of the endpoint".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_name: Option<String>,
}

/// A complete pair of ACL credentials.
///
/// The password is a secret: it is redacted in every `Debug` output and
/// never serialized. It reaches the server only inside the gRPC
/// `LoginRequest`.
#[derive(Clone, Deserialize)]
pub struct GraphLoginCredentials {
    /// The ACL user (e.g. the guardian `groot`).
    pub userid: String,
    /// The password of the user. Redacted everywhere. An absent or empty
    /// password deserializes to the empty string and is rejected by
    /// `validate` as a typed error, never accepted silently.
    #[serde(default)]
    pub password: String,
}

impl GraphLoginCredentials {
    /// The user that will be named in errors and logs.
    pub fn userid(&self) -> &str {
        &self.userid
    }

    /// The secret, for the one caller that actually performs the login.
    pub fn password(&self) -> &str {
        &self.password
    }
}

impl fmt::Debug for GraphLoginCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GraphLoginCredentials")
            .field("userid", &self.userid)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// Serialization redacts the password: a config that is logged, dumped or
/// returned by a debug endpoint cannot leak the secret.
impl Serialize for GraphLoginCredentials {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("GraphLoginCredentials", 2)?;
        state.serialize_field("userid", &self.userid)?;
        state.serialize_field("password", "[REDACTED]")?;
        state.end()
    }
}

fn default_connect_timeout_ms() -> u64 {
    DEFAULT_CONNECT_TIMEOUT_MS
}

fn default_request_timeout_ms() -> u64 {
    DEFAULT_REQUEST_TIMEOUT_MS
}

impl GraphClientConfig {
    /// Parses and validates the configuration from TOML text.
    ///
    /// This is the entry point a bootstrap uses before any connection
    /// exists: a deployment whose configuration does not validate never
    /// reaches the network.
    pub fn from_toml_str(text: &str) -> Result<Self, crate::error::GraphConfigLoadError> {
        let config: Self = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    /// Checks every structural rule, without touching network or disk.
    pub fn validate(&self) -> Result<(), GraphConfigError> {
        self.validate_endpoint()?;
        self.validate_tls()?;
        self.validate_timeouts()?;
        self.validate_credentials()?;
        Ok(())
    }

    fn validate_endpoint(&self) -> Result<(), GraphConfigError> {
        if self.endpoint.trim().is_empty() {
            return Err(GraphConfigError::MissingEndpoint);
        }
        if let Some((scheme, _)) = self.endpoint.split_once("://") {
            return Err(GraphConfigError::EndpointWithScheme {
                scheme: scheme.to_owned(),
                endpoint: self.endpoint.clone(),
            });
        }
        // IPv6 literals keep their brackets; the port always follows the
        // last colon outside them.
        let host_port = match self.endpoint.rsplit_once(':') {
            Some(pair) if !pair.1.is_empty() => pair,
            _ => {
                return Err(GraphConfigError::EndpointWithoutPort {
                    endpoint: self.endpoint.clone(),
                });
            }
        };
        let (host, port) = host_port;
        let host = host.trim_end_matches(['[', ']']);
        if host.is_empty() {
            return Err(GraphConfigError::EndpointWithoutPort {
                endpoint: self.endpoint.clone(),
            });
        }
        let port: u16 = port.parse().map_err(|_| GraphConfigError::InvalidPort {
            endpoint: self.endpoint.clone(),
            port: port.to_owned(),
        })?;
        if port == 0 {
            return Err(GraphConfigError::InvalidPort {
                endpoint: self.endpoint.clone(),
                port: port.to_string(),
            });
        }
        Ok(())
    }

    fn validate_tls(&self) -> Result<(), GraphConfigError> {
        if self.tls.enabled && self.tls.ca_cert_path.is_none() {
            return Err(GraphConfigError::TlsEnabledWithoutCaCertificate);
        }
        let has_cert = self.tls.client_cert_path.is_some();
        let has_key = self.tls.client_key_path.is_some();
        if has_cert != has_key {
            return Err(GraphConfigError::ClientIdentityIncomplete { has_cert, has_key });
        }
        Ok(())
    }

    fn validate_timeouts(&self) -> Result<(), GraphConfigError> {
        if self.connect_timeout_ms == 0 {
            return Err(GraphConfigError::ZeroTimeout {
                field: "connect_timeout_ms",
            });
        }
        if self.request_timeout_ms == 0 {
            return Err(GraphConfigError::ZeroTimeout {
                field: "request_timeout_ms",
            });
        }
        Ok(())
    }

    fn validate_credentials(&self) -> Result<(), GraphConfigError> {
        match &self.credentials {
            None => Ok(()),
            Some(credentials) => {
                if credentials.userid.trim().is_empty() {
                    return Err(GraphConfigError::MissingCredentialsField { field: "userid" });
                }
                if credentials.password.is_empty() {
                    return Err(GraphConfigError::EmptyPassword);
                }
                Ok(())
            }
        }
    }

    /// The host of the endpoint, as written (without the port).
    pub fn host(&self) -> &str {
        let end = self
            .endpoint
            .rsplit_once(':')
            .map(|(host, _)| host.len())
            .unwrap_or(self.endpoint.len());
        &self.endpoint[..end]
    }

    /// The port of the endpoint, already validated.
    pub fn port(&self) -> u16 {
        self.endpoint
            .rsplit_once(':')
            .and_then(|(_, port)| port.parse().ok())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a TOML document from its parts. Sections are spliced in
    /// order (top-level keys first), because TOML keys after a table
    /// header belong to that table and duplicate tables are errors.
    fn doc(endpoint: &str, tls: Option<&str>, credentials: Option<&str>) -> String {
        let mut text = format!("endpoint = \"{endpoint}\"\n");
        if let Some(tls) = tls {
            text.push_str(&format!("\n[tls]\n{tls}\n"));
        }
        if let Some(credentials) = credentials {
            text.push_str(&format!("\n[credentials]\n{credentials}\n"));
        }
        text
    }

    fn full_valid() -> String {
        doc(
            "127.0.0.1:9080",
            Some("enabled = true\nca_cert_path = \"/tls/ca.crt\""),
            Some("userid = \"groot\"\npassword = \"the-secret-password\""),
        )
    }

    /// Asserts that the document loads to exactly the typed configuration
    /// error: `from_toml_str` parses AND validates, so an invalid document
    /// is rejected before any probe could exist.
    fn expect_invalid(text: &str, expected: GraphConfigError) {
        match GraphClientConfig::from_toml_str(text) {
            Err(crate::error::GraphConfigLoadError::Invalid(actual)) => {
                assert_eq!(actual, expected)
            }
            other => {
                panic!("expected the typed error {expected:?}, got {other:?}")
            }
        }
    }

    #[test]
    fn a_complete_configuration_parses_and_validates() {
        let config = GraphClientConfig::from_toml_str(&full_valid()).expect("valid config");
        assert_eq!(config.endpoint, "127.0.0.1:9080");
        assert!(config.tls.enabled);
        assert_eq!(config.credentials.as_ref().unwrap().userid(), "groot");
        assert_eq!(config.connect_timeout_ms, DEFAULT_CONNECT_TIMEOUT_MS);
        assert_eq!(config.request_timeout_ms, DEFAULT_REQUEST_TIMEOUT_MS);
        assert_eq!(config.namespace, 0);
        config.validate().expect("valid config validates");
    }

    #[test]
    fn a_plaintext_choice_is_explicit_and_valid() {
        let config =
            GraphClientConfig::from_toml_str(&doc("127.0.0.1:9080", Some("enabled = false"), None))
                .expect("plaintext config");
        assert!(!config.tls.enabled);
        config.validate().expect("plaintext config validates");
    }

    #[test]
    fn a_missing_endpoint_is_a_typed_error() {
        expect_invalid(
            &doc("", Some("enabled = false"), None),
            GraphConfigError::MissingEndpoint,
        );
    }

    #[test]
    fn a_scheme_in_the_endpoint_is_rejected() {
        expect_invalid(
            &doc("https://127.0.0.1:9080", Some("enabled = false"), None),
            GraphConfigError::EndpointWithScheme {
                scheme: "https".to_owned(),
                endpoint: "https://127.0.0.1:9080".to_owned(),
            },
        );
    }

    #[test]
    fn an_endpoint_without_port_is_rejected() {
        expect_invalid(
            &doc("127.0.0.1", Some("enabled = false"), None),
            GraphConfigError::EndpointWithoutPort {
                endpoint: "127.0.0.1".to_owned(),
            },
        );
    }

    #[test]
    fn an_invalid_port_is_rejected() {
        expect_invalid(
            &doc("127.0.0.1:99999", Some("enabled = false"), None),
            GraphConfigError::InvalidPort {
                endpoint: "127.0.0.1:99999".to_owned(),
                port: "99999".to_owned(),
            },
        );
    }

    #[test]
    fn tls_without_a_ca_certificate_is_rejected() {
        expect_invalid(
            &doc("127.0.0.1:9080", Some("enabled = true"), None),
            GraphConfigError::TlsEnabledWithoutCaCertificate,
        );
    }

    #[test]
    fn a_client_certificate_without_key_is_rejected() {
        expect_invalid(
            &doc(
                "127.0.0.1:9080",
                Some(
                    "enabled = true\nca_cert_path = \"/tls/ca.crt\"\nclient_cert_path = \"/tls/client.crt\"",
                ),
                None,
            ),
            GraphConfigError::ClientIdentityIncomplete {
                has_cert: true,
                has_key: false,
            },
        );
    }

    #[test]
    fn a_zero_timeout_is_rejected() {
        let config =
            GraphClientConfig::from_toml_str(&doc("127.0.0.1:9080", Some("enabled = false"), None))
                .expect("parse works; validation must not");
        let config = GraphClientConfig {
            connect_timeout_ms: 0,
            ..config
        };
        assert_eq!(
            config.validate(),
            Err(GraphConfigError::ZeroTimeout {
                field: "connect_timeout_ms",
            })
        );
    }

    #[test]
    fn credentials_without_a_user_are_rejected() {
        let config = GraphClientConfig {
            credentials: Some(GraphLoginCredentials {
                userid: String::new(),
                password: "the-secret-password".to_owned(),
            }),
            ..GraphClientConfig::from_toml_str(&doc(
                "127.0.0.1:9080",
                Some("enabled = false"),
                None,
            ))
            .expect("base config")
        };
        assert!(matches!(
            config.validate(),
            Err(GraphConfigError::MissingCredentialsField { field: "userid" })
        ));
    }

    #[test]
    fn an_empty_password_is_rejected() {
        expect_invalid(
            &doc(
                "127.0.0.1:9080",
                Some("enabled = false"),
                Some("userid = \"groot\"\npassword = \"\""),
            ),
            GraphConfigError::EmptyPassword,
        );
    }

    #[test]
    fn a_password_omitted_from_the_document_is_rejected() {
        expect_invalid(
            &doc(
                "127.0.0.1:9080",
                Some("enabled = false"),
                Some("userid = \"groot\""),
            ),
            GraphConfigError::EmptyPassword,
        );
    }

    #[test]
    fn malformed_toml_is_a_load_error() {
        let result = GraphClientConfig::from_toml_str("endpoint = [not toml");
        assert!(matches!(
            result,
            Err(crate::error::GraphConfigLoadError::Parse { .. })
        ));
    }

    #[test]
    fn the_password_never_appears_in_debug_output() {
        let config = GraphClientConfig::from_toml_str(&full_valid()).expect("valid config");
        let rendered = format!("{config:?}");
        assert!(
            !rendered.contains("the-secret-password"),
            "the Debug of the config leaked the password: {rendered}"
        );
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn the_password_never_appears_in_serialized_output() {
        let config = GraphClientConfig::from_toml_str(&full_valid()).expect("valid config");
        let rendered = toml::to_string(&config).expect("config serializes");
        assert!(
            !rendered.contains("the-secret-password"),
            "serialization leaked the password: {rendered}"
        );
        assert!(rendered.contains("[REDACTED]"));
    }

    #[test]
    fn the_host_and_port_are_split_from_the_endpoint() {
        let config = GraphClientConfig::from_toml_str(&full_valid()).expect("valid config");
        assert_eq!(config.host(), "127.0.0.1");
        assert_eq!(config.port(), 9080);
    }
}
