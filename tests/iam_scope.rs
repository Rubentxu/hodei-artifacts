//! IAM policy objects are scoped to the verified account.

use std::sync::Arc;

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
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

struct Accounts;

#[async_trait]
impl PrincipalAuthenticator for Accounts {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        let (account, name) = match token {
            "alice" => ("acme", "alice"),
            "bob" => ("other", "bob"),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::{account}:User/{name}")).unwrap(),
            AccountId::new(account).unwrap(),
        ))
    }
}

async fn app() -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = true;
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();
    let policy = r#"permit(principal, action == Admin::Action::"AccessAdminApi", resource);"#;
    let authorizer: Arc<dyn AuthorizerPort> = Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .unwrap(),
    )));
    let state = bootstrap(
        &config,
        BootstrapConfig {
            register_iam_schema: false,
            schema_version: None,
            validate_schemas: false,
        },
    )
    .await
    .unwrap()
    .with_authenticator(Arc::new(Accounts))
    .with_authorization(authorizer);
    (build_router(state, &config), tmp)
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
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

async fn create(app: &axum::Router, token: &str, policy_id: &str) -> Value {
    let (status, body) = call(
        app,
        "POST",
        "/api/v1/iam/policies",
        token,
        Some(json!({
            "policy_id": policy_id,
            "policy_content": "permit(principal, action, resource);"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create: {body:?}");
    body
}

#[tokio::test]
async fn two_accounts_cannot_read_update_delete_or_list_each_others_policies() {
    let (app, _tmp) = app().await;

    let acme = create(&app, "alice", "shared-id").await;
    let other = create(&app, "bob", "shared-id").await;
    let acme_hrn = acme["hrn"].as_str().unwrap();
    let other_hrn = other["hrn"].as_str().unwrap();
    assert!(acme_hrn.contains(":acme:"));
    assert!(other_hrn.contains(":other:"));

    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/iam/policies/get",
        "alice",
        Some(json!({"policy_hrn": other_hrn})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "cross-account get: {body:?}");

    let (status, _) = call(
        &app,
        "PUT",
        "/api/v1/iam/policies/update",
        "alice",
        Some(json!({"policy_hrn": other_hrn, "policy_content": "permit(principal, action, resource);"})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = call(
        &app,
        "DELETE",
        "/api/v1/iam/policies/delete",
        "alice",
        Some(json!({"policy_hrn": other_hrn})),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Both policies still exist for their owners and list is scoped.
    let (status, body) = call(
        &app,
        "POST",
        "/api/v1/iam/policies/get",
        "bob",
        Some(json!({"policy_hrn": other_hrn})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "owner get: {body:?}");

    let (status, body) = call(&app, "GET", "/api/v1/iam/policies", "alice", None).await;
    assert_eq!(status, StatusCode::OK);
    let listed: Vec<&str> = body["policies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|policy| policy["hrn"].as_str().unwrap())
        .collect();
    assert_eq!(listed, vec![acme_hrn]);
}

#[tokio::test]
async fn attachments_and_evaluation_cannot_point_to_another_account() {
    let (app, _tmp) = app().await;

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        "alice",
        Some(json!({
            "policy_id": "cross-attach",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::other:User/bob"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = call(
        &app,
        "POST",
        "/api/v1/iam/evaluate",
        "alice",
        Some(json!({
            "principal_hrn": "hrn:hodei:iam::acme:User/alice",
            "action": "CreateUser",
            "resource_hrn": "hrn:hodei:iam::other:User/bob"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
