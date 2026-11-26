//! Executable recipes of HSP-243 (Maven 2, npm and PyPI) against the real service.
//!
//! The documentation checks of the same guides live in `hsp243_user_guides` and run
//! in the default build; this suite boots the application on the graph profile (a real Dgraph).

mod guide_blocks;

use async_trait::async_trait;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use base64::Engine;
use cedar_policy::Entities;
use guide_blocks::RECIPE_REQUESTS;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use hodei_catalog::RepositoryFormat;
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use std::cell::RefCell;
use std::sync::Arc;
use tower::ServiceExt;

// The graph profile of the harness: a REAL Dgraph namespace per application.
#[path = "support/graph_profile.rs"]
mod graph_profile;
use graph_profile::Workdir;

const ACCOUNT: &str = "acme";

struct TestAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for TestAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        let (account, name) = match token {
            "alice" => (ACCOUNT, "alice"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

fn cedar_authorizer() -> Arc<dyn AuthorizerPort> {
    let policy = r#"
        permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"CreateRepository",
            resource
        );
        permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"ReadRepository",
            resource
        );
        permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"UploadAsset",
            resource
        );
        permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Artifacts::Action::"DownloadAsset",
            resource
        );
    "#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("cedar authorizer"),
    )))
}

async fn real_app() -> (axum::Router, Workdir) {
    let tmp = Workdir::new();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = false;
    config.security.raw_api_enabled = true;
    tmp.apply(&mut config);
    config.content.cas_path = tmp.path().join("cas").to_string_lossy().into_owned();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap");

    let state = state
        .with_authorization(cedar_authorizer())
        .with_authenticator(Arc::new(TestAuthenticator));

    (build_router(state, &config), tmp)
}

thread_local! {
    /// `(method, path)` of every request `send` made, in order.
    static SENT: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
}

async fn send(
    app: &axum::Router,
    method: &str,
    uri: &str,
    content_type: Option<&str>,
    body: Vec<u8>,
) -> (StatusCode, Vec<u8>) {
    SENT.with(|sent| {
        sent.borrow_mut()
            .push((method.to_string(), uri.to_string()))
    });
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:8080")
        .header("authorization", "Bearer alice");
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    let req = builder.body(Body::from(body)).expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    let status = resp.status();
    let bytes = to_bytes(resp.into_body(), 10 * 1024 * 1024)
        .await
        .expect("body bytes")
        .to_vec();
    (status, bytes)
}

#[tokio::test]
async fn documented_maven_npm_pypi_recipes_work_against_real_service() {
    let (app, _tmp) = real_app().await;

    // 0. Live /api/v1/capabilities matches RepositoryFormat::served() & serves_modes().
    let (status, cap_bytes) = send(&app, "GET", "/api/v1/capabilities", None, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let cap_json: Value = serde_json::from_slice(&cap_bytes).expect("capabilities json");
    let cap_formats = cap_json["formats"].as_array().expect("formats array");
    assert_eq!(cap_formats.len(), RepositoryFormat::served().len());
    for (idx, format) in RepositoryFormat::served().iter().enumerate() {
        assert_eq!(cap_formats[idx]["name"], format.as_str());
        let expected_modes: Vec<&str> = format.serves_modes().iter().map(|m| m.as_str()).collect();
        let actual_modes: Vec<&str> = cap_formats[idx]["modes"]
            .as_array()
            .expect("modes")
            .iter()
            .map(|v| v.as_str().expect("mode str"))
            .collect();
        assert_eq!(actual_modes, expected_modes);
    }

    SENT.with(|sent| sent.borrow_mut().clear());

    // 1. Maven 2 recipe (doc-maven)
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/catalog/repositories",
        Some("application/json"),
        json!({
            "name": "doc-maven",
            "format": "maven2",
            "mode": "hosted",
            "settings": {"maven": {"version_policy": "release"}}
        })
        .to_string()
        .into_bytes(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let pom = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<project><modelVersion>4.0.0</modelVersion><groupId>com.acme</groupId><artifactId>demo</artifactId><version>1.0.0</version></project>\n";
    let (status, body) = send(
        &app,
        "PUT",
        "/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.pom",
        Some("application/xml"),
        pom.to_vec(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let jar = b"PK\x03\x04demo-jar-content";
    let (status, body) = send(
        &app,
        "PUT",
        "/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.jar",
        Some("application/java-archive"),
        jar.to_vec(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let (status, downloaded_jar) = send(
        &app,
        "GET",
        "/repository/doc-maven/com/acme/demo/1.0.0/demo-1.0.0.jar",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(downloaded_jar, jar);

    // 2. npm recipe (doc-npm)
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/catalog/repositories",
        Some("application/json"),
        json!({"name": "doc-npm", "format": "npm", "mode": "hosted"})
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let tarball = b"demo-npm-tarball-bytes";
    let integrity = format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(Sha512::digest(tarball))
    );
    let npm_payload = json!({
        "_id": "@acme/demo",
        "name": "@acme/demo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": {
                "name": "@acme/demo",
                "version": "1.0.0",
                "_id": "@acme/demo@1.0.0",
                "dist": { "integrity": integrity }
            }
        },
        "_attachments": {
            "@acme/demo-1.0.0.tgz": {
                "content_type": "application/octet-stream",
                "data": base64::engine::general_purpose::STANDARD.encode(tarball),
                "length": tarball.len(),
            }
        }
    });
    let (status, body) = send(
        &app,
        "PUT",
        "/npm/doc-npm/@acme%2fdemo",
        Some("application/json"),
        npm_payload.to_string().into_bytes(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let (status, packument) =
        send(&app, "GET", "/npm/doc-npm/@acme%2fdemo", None, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let packument_json: Value = serde_json::from_slice(&packument).expect("packument");
    assert_eq!(packument_json["dist-tags"]["latest"], "1.0.0");

    let (status, downloaded_tgz) = send(
        &app,
        "GET",
        "/npm/doc-npm/@acme/demo/-/demo-1.0.0.tgz",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(downloaded_tgz, tarball);

    // 3. PyPI recipe (doc-pypi)
    let (status, body) = send(
        &app,
        "POST",
        "/api/v1/catalog/repositories",
        Some("application/json"),
        json!({"name": "doc-pypi", "format": "pypi", "mode": "hosted"})
            .to_string()
            .into_bytes(),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "{}",
        String::from_utf8_lossy(&body)
    );

    let wheel = b"PK\x03\x04demo-wheel-bytes";
    let sha256_hex: String = Sha256::digest(wheel)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let boundary = "----hsp243boundary";
    let mut multipart = Vec::new();
    for (field, value) in [
        (":action", "file_upload"),
        ("protocol_version", "1"),
        ("name", "demo-pkg"),
        ("version", "1.0.0"),
        ("metadata_version", "2.1"),
        ("sha256_digest", sha256_hex.as_str()),
    ] {
        multipart.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{field}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    multipart.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"content\"; filename=\"demo_pkg-1.0.0-py3-none-any.whl\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .as_bytes(),
    );
    multipart.extend_from_slice(wheel);
    multipart.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());

    let (status, body) = send(
        &app,
        "POST",
        "/pypi/doc-pypi/",
        Some(&format!("multipart/form-data; boundary={boundary}")),
        multipart,
    )
    .await;
    assert!(
        status == StatusCode::OK || status == StatusCode::CREATED,
        "pypi upload failed with {status}: {}",
        String::from_utf8_lossy(&body)
    );

    let (status, simple_html) = send(
        &app,
        "GET",
        "/pypi/doc-pypi/simple/demo-pkg/",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let html_str = String::from_utf8_lossy(&simple_html);
    assert!(
        html_str.contains("demo_pkg-1.0.0-py3-none-any.whl"),
        "simple index missing wheel: {html_str}"
    );

    // 4. The wheel linked by the index downloads with the exact bytes uploaded.
    let (status, downloaded_wheel) = send(
        &app,
        "GET",
        "/pypi/doc-pypi/files/demo-pkg/demo_pkg-1.0.0-py3-none-any.whl",
        None,
        Vec::new(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(downloaded_wheel, wheel, "the downloaded wheel differs");
    assert!(
        html_str.contains("/pypi/doc-pypi/files/demo-pkg/demo_pkg-1.0.0-py3-none-any.whl#sha256=")
            && html_str.contains(&sha256_hex),
        "the index must link the wheel with its sha256: {html_str}"
    );

    let sent = SENT.with(|sent| sent.borrow().clone());
    let expected: Vec<(String, String)> = RECIPE_REQUESTS
        .iter()
        .map(|(_, method, path)| (method.to_string(), path.to_string()))
        .collect();
    assert_eq!(sent, expected, "the recipe sent unexpected requests");
}
