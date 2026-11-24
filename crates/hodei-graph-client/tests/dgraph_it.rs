//! Integration tests against a REAL local Dgraph server (AC#1 and AC#2).
//!
//! These tests are environment-gated: without
//! `HODEI_GRAPH_IT_ENDPOINT` they skip (exit 0), so ordinary CI runs stay
//! hermetic. The full AC#1 run needs one Zero+Alpha with ACL and TLS and
//! the variables below:
//!
//! - `HODEI_GRAPH_IT_ENDPOINT`  gRPC endpoint of the Alpha, `host:port`.
//! - `HODEI_GRAPH_IT_CA`        path to the cluster CA certificate (PEM).
//! - `HODEI_GRAPH_IT_SERVER_NAME` optional DNS name to verify the server
//!   certificate against (the SAN of the node certificate), when the
//!   Alpha is reached by address.
//! - `HODEI_GRAPH_IT_USER`      ACL guardian user (e.g. `groot`).
//! - `HODEI_GRAPH_IT_PASSWORD`  password of that user.
//!
//! The tests assert behavior, not infrastructure: an authenticated,
//! TLS-ready, timeout-bounded session succeeds; a wrong endpoint, a dead
//! port and a wrong password fail with the corresponding typed errors.
//! Nothing here logs credentials: the failures are printed through the
//! error types, which redact secrets by construction.

use hodei_graph_client::{GraphClient, GraphClientConfig, GraphClientError, GraphHealthPort};

/// The TOML configuration built from the environment, or `None` when the
/// suite is not being run against a server.
fn config_from_env() -> Option<String> {
    let endpoint = std::env::var("HODEI_GRAPH_IT_ENDPOINT").ok()?;
    let ca = std::env::var("HODEI_GRAPH_IT_CA").unwrap_or_default();
    let server_name = std::env::var("HODEI_GRAPH_IT_SERVER_NAME").unwrap_or_default();
    let user = std::env::var("HODEI_GRAPH_IT_USER").unwrap_or_default();
    let password = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();

    let mut text = format!(
        "endpoint = \"{endpoint}\"\n\n[tls]\nenabled = {enabled}\nca_cert_path = \"{ca}\"\n",
        enabled = !ca.is_empty()
    );
    if !server_name.is_empty() {
        text.push_str(&format!("server_name = \"{server_name}\"\n"));
    }
    if !user.is_empty() {
        text.push_str(&format!(
            "\n[credentials]\nuserid = \"{user}\"\npassword = \"{password}\"\n"
        ));
    }
    Some(text)
}

#[tokio::test]
async fn ac1_authenticated_tls_ready_session_against_the_real_server() {
    let Some(text) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set; the real-server run is gated");
        return;
    };
    let config = GraphClientConfig::from_toml_str(&text)
        .expect("the environment configuration must validate");
    let mut client = GraphClient::connect(config)
        .await
        .expect("connect with TLS inside the connect timeout");

    // The readiness probe of the port trait: connect already happened, the
    // probe performs login, version check and the DQL round-trip.
    let readiness = client
        .ready()
        .await
        .expect("the real server must be ready for an authenticated session");
    eprintln!("readiness: {readiness:?}");
    assert!(
        readiness.authenticated_query_served,
        "the DQL round-trip must be served"
    );
    assert!(readiness.login_performed, "the probe must have logged in");

    // The explicit login of the client itself, so the session below is
    // authenticated too (the readiness probe used its own clone).
    client
        .login()
        .await
        .expect("the ACL login must succeed with the rotated guardian password");

    // One real DQL query of the demo fixture, through the query port.
    let response = client
        .execute_dql("schema {}")
        .await
        .expect("schema query must succeed");
    assert!(response.json.is_object(), "the schema answer must be JSON");

    // The version tag proves the login and the channel are alive.
    eprintln!("server reported readiness: {readiness:?}");
}

#[tokio::test]
async fn ac2_a_wrong_endpoint_fails_explicitly() {
    let Some(text) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set; the real-server run is gated");
        return;
    };
    // Same TLS material and credentials as the happy path, but a port
    // nothing listens on: the failure must be typed and explicit.
    let wrong = text.replace(
        &std::env::var("HODEI_GRAPH_IT_ENDPOINT").expect("checked above"),
        "127.0.0.1:1",
    );
    let config =
        GraphClientConfig::from_toml_str(&wrong).expect("the wrong-endpoint config must validate");
    let error = GraphClient::connect(config)
        .await
        .expect_err("a closed port must fail explicitly");
    assert!(
        matches!(
            error,
            GraphClientError::Connect { .. } | GraphClientError::ConnectTimeout { .. }
        ),
        "unexpected error: {error}"
    );
    eprintln!("wrong endpoint produced: {error}");
}

#[tokio::test]
async fn ac2_invalid_auth_fails_explicitly() {
    let Some(text) = config_from_env() else {
        eprintln!("SKIP: HODEI_GRAPH_IT_ENDPOINT is not set; the real-server run is gated");
        return;
    };
    // A wrong password: the config is structurally valid, the server is
    // real, and the rejection must be a typed login failure whose message
    // carries no credentials.
    let wrong = text.replace("password = \"", "password = \"wrong-");
    let config =
        GraphClientConfig::from_toml_str(&wrong).expect("the wrong-password config must validate");
    let mut client = GraphClient::connect(config)
        .await
        .expect("the channel must connect; only the credentials are wrong");
    let error = client
        .login()
        .await
        .expect_err("a wrong password must be rejected");
    assert!(
        matches!(error, GraphClientError::LoginRejected { .. }),
        "unexpected error: {error}"
    );
    let rendered = error.to_string();
    let secret = std::env::var("HODEI_GRAPH_IT_PASSWORD").unwrap_or_default();
    // The server's own rejection text ("invalid username or password") is
    // legitimate and may contain a weak password as a substring (the
    // default guardian password is literally `password`). Remove that
    // fixed boilerplate before looking for an echo of the credentials, so
    // the check is about leaks and not about English words.
    let without_server_text = rendered.replace("invalid username or password", "");
    assert!(
        secret.is_empty() || !without_server_text.contains(&secret),
        "the login failure leaked the password: {rendered}"
    );
    assert!(
        !without_server_text.contains("wrong-"),
        "the login failure echoed the wrong password sent: {rendered}"
    );
    eprintln!("invalid auth produced: {rendered}");
}
