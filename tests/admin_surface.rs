//! Security contract for the administrative HTTP surface.
//!
//! The IAM/policy/schema routes are administrative. This service does not
//! authenticate requests yet, so the default configuration must not expose
//! them. The tests drive the real router in-process, without binding a port.

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
use tempfile::TempDir;
use tower::ServiceExt;

const ADMIN_ROUTES: &[(&str, &str)] = &[
    ("POST", "/api/v1/schemas/build"),
    ("GET", "/api/v1/schemas/load"),
    ("POST", "/api/v1/schemas/register-iam"),
    ("POST", "/api/v1/policies/validate"),
    ("POST", "/api/v1/policies/evaluate"),
    ("POST", "/api/v1/iam/policies"),
    ("GET", "/api/v1/iam/policies"),
    ("POST", "/api/v1/iam/policies/get"),
    ("PUT", "/api/v1/iam/policies/update"),
    ("DELETE", "/api/v1/iam/policies/delete"),
    ("POST", "/api/v1/iam/evaluate"),
    ("POST", "/api/v1/playground/evaluate"),
];

const GATE_MARKER: &str = "administrative API disabled";

async fn test_app() -> (Router, TempDir) {
    test_app_with(false, Vec::new()).await
}

async fn test_app_with(
    admin_enabled: bool,
    cors_allowed_origins: Vec<String>,
) -> (Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");

    let mut config = AppConfig::default();
    config.security.admin_api_enabled = admin_enabled;
    config.security.cors_allowed_origins = cors_allowed_origins;
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
    .expect("bootstrap must succeed");

    let app = build_router(state, &config);
    (app, tmp)
}

struct AdminAuthenticator;

#[async_trait]
impl PrincipalAuthenticator for AdminAuthenticator {
    async fn authenticate(
        &self,
        credentials: &Credentials,
    ) -> Result<PrincipalClaims, AuthenticationError> {
        let token = match credentials {
            Credentials::Bearer(token) => token.as_str(),
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        let name = match token {
            "alice" => "alice",
            "bob" => "bob",
            _ => return Err(AuthenticationError::InvalidCredentials),
        };
        Ok(PrincipalClaims::new(
            PrincipalId::new(format!("hrn:hodei:iam::acme:User/{name}")).unwrap(),
            AccountId::new("acme").unwrap(),
        ))
    }
}

fn authorizer_with_policy(policy: &str) -> Arc<dyn AuthorizerPort> {
    Arc::new(SharedAuthorizer::new(Arc::new(
        CedarAuthorizer::from_policies_unchecked(
            policy,
            Entities::empty(),
            Arc::new(HrnCedarMapper),
        )
        .expect("authorizer"),
    )))
}

async fn test_app_with_admin_policy(admin_enabled: bool, policy: &str) -> (Router, TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut config = AppConfig::default();
    config.security.admin_api_enabled = admin_enabled;
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
    .expect("bootstrap")
    .with_authenticator(Arc::new(AdminAuthenticator))
    .with_authorization(authorizer_with_policy(policy));
    (build_router(state, &config), tmp)
}

async fn test_app_with_admin_auth(permit_alice: bool) -> (Router, TempDir) {
    let policy = if permit_alice {
        r#"permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Admin::Action::"AccessAdminApi",
            resource
        );"#
    } else {
        ""
    };
    test_app_with_admin_policy(true, policy).await
}

async fn response_of_with_token(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from("{}")).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn response_of(app: &Router, method: &str, uri: &str) -> (StatusCode, String) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("request");

    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn administrative_routes_are_closed_by_default() {
    let (app, _tmp) = test_app().await;

    for (method, uri) in ADMIN_ROUTES {
        let (status, body) = response_of(&app, method, uri).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{method} {uri} must not be reachable without authentication"
        );
        assert_eq!(
            body, GATE_MARKER,
            "{method} {uri} must be stopped by the gate before the handler runs"
        );
    }
}

#[tokio::test]
async fn health_probes_remain_public() {
    let (app, _tmp) = test_app().await;

    for uri in ["/health", "/health/ready", "/health/live"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK, "{uri} must stay public");
    }
}

#[tokio::test]
async fn cors_does_not_allow_arbitrary_origins_by_default() {
    let (app, _tmp) = test_app().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .header(header::ORIGIN, "https://evil.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "the default CORS policy must not authorize arbitrary origins"
    );
}

#[tokio::test]
async fn administrative_routes_require_a_configured_authenticator_and_authorizer() {
    let (app, _tmp) = test_app_with(true, Vec::new()).await;
    for (method, uri) in ADMIN_ROUTES {
        let (status, body) = response_of(&app, method, uri).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}");
        assert_eq!(body, "administrative API requires authentication");
    }
}

#[tokio::test]
async fn anonymous_requests_are_401_when_admin_auth_is_configured() {
    let (app, _tmp) = test_app_with_admin_auth(true).await;
    for (method, uri) in ADMIN_ROUTES {
        let (status, _body) = response_of_with_token(&app, method, uri, None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
}

#[tokio::test]
async fn anonymous_admin_request_carries_a_bearer_challenge() {
    let (app, _tmp) = test_app_with_admin_auth(true).await;
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/schemas/load")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.headers().get(header::WWW_AUTHENTICATE).unwrap(),
        "Bearer"
    );
}

#[tokio::test]
async fn admin_flag_stays_required_when_auth_is_configured() {
    let (app, _tmp) = test_app_with_admin_policy(
        false,
        r#"permit(principal, action == Admin::Action::"AccessAdminApi", resource);"#,
    )
    .await;
    let (status, body) =
        response_of_with_token(&app, "GET", "/api/v1/schemas/load", Some("alice")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "administrative API disabled");
}

#[tokio::test]
async fn invalid_admin_token_is_401() {
    let (app, _tmp) = test_app_with_admin_auth(true).await;
    let (status, _) =
        response_of_with_token(&app, "GET", "/api/v1/schemas/load", Some("invalid")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn a_permit_for_one_method_and_path_does_not_grant_another() {
    let (app, _tmp) = test_app_with_admin_policy(
        true,
        r#"permit(
            principal == Iam::User::"hrn:hodei:iam::acme:User/alice",
            action == Admin::Action::"AccessAdminApi",
            resource == Admin::AdminApi::"hrn:hodei:admin:global:acme:AdminApi/get-iam-policies"
        );"#,
    )
    .await;

    let (get_status, _) =
        response_of_with_token(&app, "GET", "/api/v1/iam/policies", Some("alice")).await;
    assert_eq!(get_status, StatusCode::OK);

    let (post_status, _) =
        response_of_with_token(&app, "POST", "/api/v1/iam/policies", Some("alice")).await;
    assert_eq!(
        post_status,
        StatusCode::FORBIDDEN,
        "a GET permit must not grant POST on the same path"
    );
}

#[tokio::test]
async fn identity_without_admin_permission_is_403() {
    let (app, _tmp) = test_app_with_admin_auth(true).await;
    let (status, body) =
        response_of_with_token(&app, "GET", "/api/v1/schemas/load", Some("bob")).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body, "administrative permission denied");
}

#[tokio::test]
async fn permitted_admin_reaches_schemas_and_playground() {
    let (app, _tmp) = test_app_with_admin_auth(true).await;

    let (schemas_status, _) =
        response_of_with_token(&app, "GET", "/api/v1/schemas/load", Some("alice")).await;
    assert_eq!(schemas_status, StatusCode::OK);

    let (playground_status, _) =
        response_of_with_token(&app, "POST", "/api/v1/playground/evaluate", Some("alice")).await;
    assert!(
        playground_status.is_success()
            || playground_status == StatusCode::BAD_REQUEST
            || playground_status == StatusCode::UNPROCESSABLE_ENTITY,
        "a permitted admin must reach the playground handler, got {playground_status}"
    );
}

#[tokio::test]
async fn cors_allows_only_the_configured_origins() {
    // The trailing slash is normalized: browsers never send it in `Origin`.
    let (app, _tmp) = test_app_with(false, vec!["https://app.example/".to_string()]).await;

    let allowed = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/health")
                .header(header::ORIGIN, "https://app.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(
        allowed
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .expect("allowed origin header"),
        "https://app.example"
    );

    let denied = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .header(header::ORIGIN, "https://evil.example")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert!(
        denied
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none(),
        "an origin outside the allowlist must not be authorized"
    );
}

#[tokio::test]
async fn wildcard_in_unvalidated_config_keeps_cors_closed() {
    // `build_router` is public and callers may skip `AppConfig::validate`.
    for wildcard in ["*", "*/"] {
        let (app, _tmp) = test_app_with(false, vec![wildcard.to_string()]).await;

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none(),
            "an unvalidated wildcard ({wildcard}) must not reopen CORS"
        );
    }
}

#[tokio::test]
async fn allowed_cors_origin_does_not_open_the_admin_surface() {
    let (app, _tmp) = test_app_with(false, vec!["https://app.example".to_string()]).await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/iam/policies")
                .header(header::ORIGIN, "https://app.example")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
        "https://app.example"
    );
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    assert_eq!(&body[..], GATE_MARKER.as_bytes());
}

#[tokio::test]
async fn repository_route_uses_its_own_auth_gate() {
    for admin_enabled in [false, true] {
        let (app, _tmp) = test_app_with(admin_enabled, Vec::new()).await;
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/repositories?hrn=hrn%3Ahodei%3Aartifacts%3Aeu%3Aacme%3Arepository%2Freleases")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "the repository route must require its own credential regardless of admin_api_enabled"
        );
    }
}
