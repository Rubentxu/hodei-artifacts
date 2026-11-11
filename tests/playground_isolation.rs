//! The playground must not become the production authorization authority.
//!
//! These tests drive the real router in-process. They prove that:
//! - the playground route is administrative and closed by default;
//! - an inline evaluation does not create or mutate operational policies;
//! - a missing policy or a schema mismatch never produces an `ALLOW`.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use cedar_policy::Entities;
use hodei_artifacts_api::authorization::{AuthorizerPort, SharedAuthorizer};
use hodei_artifacts_api::{AppConfig, BootstrapConfig, bootstrap, router::build_router};
use hodei_authz::cedar::CedarAuthorizer;
use hodei_authz::hrn_mapper::HrnCedarMapper;
use hodei_authz::{
    AccountId, AuthenticationError, Credentials, PrincipalAuthenticator, PrincipalClaims,
    PrincipalId,
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tower::ServiceExt;

struct PlaygroundAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for PlaygroundAuthenticator {
    async fn authenticate(
        &self,
        _credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        Ok(PrincipalClaims::new(
            PrincipalId::new("hrn:hodei:iam::acme:User/playground-admin").unwrap(),
            AccountId::new("acme").unwrap(),
        ))
    }
}

fn playground_admin_authorizer() -> Arc<dyn AuthorizerPort> {
    let policy = r#"permit(
        principal,
        action == Admin::Action::"AccessAdminApi",
        resource
    );"#;
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

async fn test_app(admin_enabled: bool) -> (Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = admin_enabled;
    config.rocksdb.path = tmp
        .path()
        .join("hodei.rocksdb")
        .to_string_lossy()
        .into_owned();

    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .expect("bootstrap must succeed")
    .with_authenticator(Arc::new(PlaygroundAuthenticator))
    .with_authorization(playground_admin_authorizer());
    (build_router(state, &config), tmp)
}

async fn post_json(app: &Router, uri: &str, body: Value) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(uri)
                .header(header::AUTHORIZATION, "Bearer playground-admin")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

async fn get_json(app: &Router, uri: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(uri)
                .header(header::AUTHORIZATION, "Bearer playground-admin")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json)
}

fn playground_body(policies: Vec<String>, schema: &str) -> Value {
    json!({
        "inline_schema": schema,
        "inline_policies": policies,
        "request": {
            "principal": "hrn:hodei:iam::acme:User/alice",
            "action": "Iam::Action::\"ReadRepository\"",
            "resource": "hrn:hodei:artifacts:eu:acme:repository/releases",
            "context": {}
        }
    })
}

#[tokio::test]
async fn playground_is_closed_by_default() {
    let (app, _tmp) = test_app(false).await;
    let (status, _) = post_json(
        &app,
        "/api/v1/playground/evaluate",
        playground_body(vec![], "{}"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn inline_playground_evaluation_does_not_touch_operational_policies() {
    let (app, _tmp) = test_app(true).await;

    let (before_status, before) = get_json(&app, "/api/v1/iam/policies").await;
    assert_eq!(before_status, StatusCode::OK);

    let (status, result) = post_json(
        &app,
        "/api/v1/playground/evaluate",
        playground_body(
            vec!["permit(principal, action, resource);".to_string()],
            "{}",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "playground result: {result:?}");
    assert_eq!(result["decision"], "ALLOW");

    let (after_status, after) = get_json(&app, "/api/v1/iam/policies").await;
    assert_eq!(after_status, StatusCode::OK);
    assert_eq!(
        before, after,
        "the playground must not create or mutate operational policies"
    );
}

#[tokio::test]
async fn missing_policy_never_grants_access() {
    let (app, _tmp) = test_app(true).await;
    let (status, result) = post_json(
        &app,
        "/api/v1/playground/evaluate",
        playground_body(vec![], "{}"),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an empty policy set is rejected before evaluation"
    );
    assert!(
        result.get("decision").is_none(),
        "a rejected command must not carry an ALLOW decision"
    );
}

#[tokio::test]
async fn schema_mismatch_never_grants_access() {
    let (app, _tmp) = test_app(true).await;
    let (status, result) = post_json(
        &app,
        "/api/v1/playground/evaluate",
        playground_body(
            vec!["permit(principal == Iam::User::\"alice\", action, resource);".to_string()],
            "{}",
        ),
    )
    .await;

    assert!(
        status.is_success() || status == StatusCode::BAD_REQUEST,
        "unexpected status: {status}"
    );
    assert_ne!(
        result["decision"], "ALLOW",
        "a policy over an undeclared entity type must not grant"
    );
}
