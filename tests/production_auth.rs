//! Production bootstrap with a real JWT verifier and Cedar policy bundle.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::Serialize;
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

const SECRET: &[u8] = b"production-test-secret-that-is-long-enough";
const ISSUER: &str = "https://issuer.example";
const AUDIENCE: &str = "hodei-artifacts";

const TEST_RSA_PRIVATE_KEY: &str = include_str!("fixtures/rsa-private.pem");
const TEST_JWKS: &str = include_str!("fixtures/jwks.json");

const SCHEMA: &str = r#"{
  "": { "entityTypes": {}, "actions": {} },
  "Iam": {
    "entityTypes": { "User": { "shape": { "type": "Record", "attributes": {} } } },
    "actions": {}
  },
  "Artifacts": {
    "entityTypes": { "Repository": { "shape": { "type": "Record", "attributes": {} } } },
    "actions": {
      "CreateRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } },
      "ReadRepository": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Artifacts::Repository"] } }
    }
  },
  "Admin": {
    "entityTypes": { "AdminApi": { "shape": { "type": "Record", "attributes": {} } } },
    "actions": {
      "AccessAdminApi": { "appliesTo": { "principalTypes": ["Iam::User"], "resourceTypes": ["Admin::AdminApi"] } }
    }
  }
}"#;

const POLICY: &str = r#"
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
    action == Admin::Action::"AccessAdminApi",
    resource
);
"#;

#[derive(Serialize)]
struct Claims {
    sub: String,
    account: String,
    exp: usize,
    iss: String,
    aud: String,
}

fn rs256_token(sub: &str, account: &str, audience: &str, exp: usize) -> String {
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("test-key".to_string());
    encode(
        &header,
        &Claims {
            sub: sub.to_string(),
            account: account.to_string(),
            exp,
            iss: ISSUER.to_string(),
            aud: audience.to_string(),
        },
        &EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_KEY.as_bytes()).expect("rsa key"),
    )
    .expect("token")
}

fn token(sub: &str, account: &str, audience: &str, exp: usize) -> String {
    encode(
        &Header::new(Algorithm::HS256),
        &Claims {
            sub: sub.to_string(),
            account: account.to_string(),
            exp,
            iss: ISSUER.to_string(),
            aud: audience.to_string(),
        },
        &EncodingKey::from_secret(SECRET),
    )
    .expect("token")
}

async fn production_jwks_app() -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let policy_path = tmp.path().join("policies.cedar");
    let schema_path = tmp.path().join("schema.json");
    let jwks_path = tmp.path().join("jwks.json");
    std::fs::write(&policy_path, POLICY).unwrap();
    std::fs::write(&schema_path, SCHEMA).unwrap();
    std::fs::write(&jwks_path, TEST_JWKS).unwrap();

    let mut config = AppConfig::default();
    config.security.admin_api_enabled = true;
    config.security.auth_jwks_path = Some(jwks_path.to_string_lossy().into_owned());
    config.security.auth_issuer = Some(ISSUER.to_string());
    config.security.auth_audience = Some(AUDIENCE.to_string());
    config.security.authorization_policy_path = Some(policy_path.to_string_lossy().into_owned());
    config.security.authorization_schema_path = Some(schema_path.to_string_lossy().into_owned());
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("jwks bootstrap");
    (build_router(state, &config), tmp)
}

async fn production_app() -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let policy_path = tmp.path().join("policies.cedar");
    let schema_path = tmp.path().join("schema.json");
    std::fs::write(&policy_path, POLICY).unwrap();
    std::fs::write(&schema_path, SCHEMA).unwrap();

    let mut config = AppConfig::default();
    config.security.admin_api_enabled = true;
    config.security.auth_jwt_secret = Some(String::from_utf8(SECRET.to_vec()).unwrap());
    config.security.auth_issuer = Some(ISSUER.to_string());
    config.security.auth_audience = Some(AUDIENCE.to_string());
    config.security.authorization_policy_path = Some(policy_path.to_string_lossy().into_owned());
    config.security.authorization_schema_path = Some(schema_path.to_string_lossy().into_owned());
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("production bootstrap");
    (build_router(state, &config), tmp)
}

async fn request(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(match body {
            Some(body) => builder
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => builder.body(Body::empty()).unwrap(),
        })
        .await
        .unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn jwt_and_cedar_bootstrap_serves_authenticated_crud_and_admin() {
    let (app, _tmp) = production_app().await;
    let alice = token(
        "hrn:hodei:iam::acme:User/alice",
        "acme",
        AUDIENCE,
        4_000_000_000,
    );

    let (created_status, created) = request(
        &app,
        "POST",
        "/api/v1/repositories",
        Some(&alice),
        Some(json!({"name": "releases", "region": "eu"})),
    )
    .await;
    assert_eq!(created_status, StatusCode::CREATED, "body: {created:?}");
    let hrn = created["hrn"].as_str().expect("hrn");

    let (read_status, read) = request(
        &app,
        "GET",
        &format!("/api/v1/repositories?hrn={hrn}"),
        Some(&alice),
        None,
    )
    .await;
    assert_eq!(read_status, StatusCode::OK);
    assert_eq!(read["name"], "releases");

    let (admin_status, _) = request(&app, "GET", "/api/v1/schemas/load", Some(&alice), None).await;
    assert_eq!(admin_status, StatusCode::OK);
}

#[tokio::test]
async fn valid_token_without_policy_is_403() {
    let (app, _tmp) = production_app().await;
    let bob = token(
        "hrn:hodei:iam::acme:User/bob",
        "acme",
        AUDIENCE,
        4_000_000_000,
    );
    let (status, _) = request(&app, "GET", "/api/v1/schemas/load", Some(&bob), None).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn invalid_audience_expired_and_bad_signature_are_401() {
    let (app, _tmp) = production_app().await;
    let wrong_audience = token(
        "hrn:hodei:iam::acme:User/alice",
        "acme",
        "other-audience",
        4_000_000_000,
    );
    let expired = token("hrn:hodei:iam::acme:User/alice", "acme", AUDIENCE, 1);
    let mut bad_signature = token(
        "hrn:hodei:iam::acme:User/alice",
        "acme",
        AUDIENCE,
        4_000_000_000,
    );
    bad_signature.push('x');

    for token in [wrong_audience, expired, bad_signature] {
        let (status, _) = request(&app, "GET", "/api/v1/schemas/load", Some(&token), None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "token must be rejected");
    }
}

#[tokio::test]
async fn partial_security_configuration_fails_bootstrap() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.security.auth_jwt_secret = Some(String::from_utf8(SECRET.to_vec()).unwrap());
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();

    let result = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await;
    assert!(
        result.is_err(),
        "a partial security bundle must fail closed"
    );
}

#[tokio::test]
async fn mismatched_token_account_is_rejected() {
    let (app, _tmp) = production_app().await;
    // The subject belongs to other, but the claim says acme.
    let forged = token(
        "hrn:hodei:iam::other:User/alice",
        "acme",
        AUDIENCE,
        4_000_000_000,
    );
    let (status, _) = request(&app, "GET", "/api/v1/schemas/load", Some(&forged), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn admin_requires_jwt_even_when_enabled() {
    let (app, _tmp) = production_app().await;
    // The production app has admin enabled; the JWT is still required at the
    // admin gate (no token -> 401).
    let (status, _) = request(&app, "GET", "/api/v1/schemas/load", None, None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn jwks_rs256_authenticates_and_hs256_is_rejected() {
    let (app, _tmp) = production_jwks_app().await;
    let alice = rs256_token(
        "hrn:hodei:iam::acme:User/alice",
        "acme",
        AUDIENCE,
        4_000_000_000,
    );
    let (status, _) = request(&app, "GET", "/api/v1/schemas/load", Some(&alice), None).await;
    assert_eq!(status, StatusCode::OK);

    let hs256 = token(
        "hrn:hodei:iam::acme:User/alice",
        "acme",
        AUDIENCE,
        4_000_000_000,
    );
    let (status, _) = request(&app, "GET", "/api/v1/schemas/load", Some(&hs256), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invalid_issuer_and_invalid_cedar_bundle_fail_closed() {
    let (app, _tmp) = production_app().await;
    // Build a token with a different issuer using the same claims shape.
    #[derive(Serialize)]
    struct WrongIssuerClaims {
        sub: String,
        account: String,
        exp: usize,
        iss: String,
        aud: String,
    }
    let wrong = encode(
        &Header::new(Algorithm::HS256),
        &WrongIssuerClaims {
            sub: "hrn:hodei:iam::acme:User/alice".to_string(),
            account: "acme".to_string(),
            exp: 4_000_000_000,
            iss: "https://evil.example".to_string(),
            aud: AUDIENCE.to_string(),
        },
        &EncodingKey::from_secret(SECRET),
    )
    .unwrap();
    let (status, _) = request(&app, "GET", "/api/v1/schemas/load", Some(&wrong), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let tmp = tempfile::tempdir().unwrap();
    let policy_path = tmp.path().join("bad-policy.cedar");
    let schema_path = tmp.path().join("schema.json");
    std::fs::write(&policy_path, "not cedar").unwrap();
    std::fs::write(&schema_path, SCHEMA).unwrap();
    let mut config = AppConfig::default();
    config.security.auth_jwt_secret = Some(String::from_utf8(SECRET.to_vec()).unwrap());
    config.security.auth_issuer = Some(ISSUER.to_string());
    config.security.auth_audience = Some(AUDIENCE.to_string());
    config.security.authorization_policy_path = Some(policy_path.to_string_lossy().into_owned());
    config.security.authorization_schema_path = Some(schema_path.to_string_lossy().into_owned());
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();
    let result = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await;
    assert!(result.is_err(), "invalid Cedar must fail bootstrap");
}

#[tokio::test]
async fn empty_secret_and_invalid_schema_fail_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let policy_path = tmp.path().join("policies.cedar");
    let schema_path = tmp.path().join("schema.json");
    std::fs::write(&policy_path, POLICY).unwrap();
    std::fs::write(&schema_path, "{not-json").unwrap();

    let mut config = AppConfig::default();
    config.security.auth_jwt_secret = Some(String::new());
    config.security.auth_issuer = Some(ISSUER.to_string());
    config.security.auth_audience = Some(AUDIENCE.to_string());
    config.security.authorization_policy_path = Some(policy_path.to_string_lossy().into_owned());
    config.security.authorization_schema_path = Some(schema_path.to_string_lossy().into_owned());
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();
    assert!(
        bootstrap(
            &config,
            BootstrapConfig {
                register_iam_schema: false,
                schema_version: None,
                validate_schemas: false,
            },
        )
        .await
        .is_err()
    );

    config.security.auth_jwt_secret = Some(String::from_utf8(SECRET.to_vec()).unwrap());
    assert!(
        bootstrap(
            &config,
            BootstrapConfig {
                register_iam_schema: false,
                schema_version: None,
                validate_schemas: false,
            },
        )
        .await
        .is_err(),
        "invalid schema must fail bootstrap"
    );
}
