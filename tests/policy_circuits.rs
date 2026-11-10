//! The static gate bundle and the persisted IAM evaluator are separate
//! authorities. A persisted administrative policy must not silently become an
//! effective permission for repository requests.

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

struct Alice;

#[async_trait]
impl PrincipalAuthenticator for Alice {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        match credentials {
            Credentials::Bearer(token) if token == "alice" => Ok(PrincipalClaims::new(
                PrincipalId::new("hrn:hodei:iam::acme:User/alice").unwrap(),
                AccountId::new("acme").unwrap(),
            )),
            _ => Err(AuthenticationError::InvalidCredentials),
        }
    }
}

async fn app() -> (axum::Router, TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = true;
    config.rocksdb.path = tmp.path().join("db").to_string_lossy().into_owned();

    // Static bundle: it only authorizes the admin surface, not repositories.
    let policy = r#"permit(
        principal,
        action == Admin::Action::"AccessAdminApi",
        resource
    );"#;
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
    .with_authenticator(Arc::new(Alice))
    .with_authorization(authorizer);
    (build_router(state, &config), tmp)
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", "Bearer alice");
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
async fn persisted_iam_policy_does_not_change_the_static_gate() {
    let (app, _tmp) = app().await;

    let (status, created) = call(
        &app,
        "POST",
        "/api/v1/iam/policies",
        Some(json!({
            "policy_id": "diagnostic",
            "policy_content": "permit(principal, action, resource);",
            "attached_principals": ["hrn:hodei:iam::acme:User/alice"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "create: {created:?}");

    // The persisted evaluator can grant its diagnostic decision...
    let (status, decision) = call(
        &app,
        "POST",
        "/api/v1/iam/evaluate",
        Some(json!({
            "principal_hrn": "hrn:hodei:iam::acme:User/alice",
            "action": "CreateUser",
            "resource_hrn": "hrn:hodei:iam::acme:User/bob"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(decision["decision"], json!(true));

    // ...but it is not the authority for the repository gate.
    let (status, _) = call(
        &app,
        "GET",
        "/api/v1/repositories?hrn=hrn%3Ahodei%3Aartifacts%3Aeu%3Aacme%3Arepository%2Freleases",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
