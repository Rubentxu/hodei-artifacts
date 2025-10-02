use axum::{body::Body, http::{Request, StatusCode}};
use serde_json::json;
use tower::ServiceExt;

use hodei_artifacts_api::build_app_for_tests;

async fn build_test_app() -> axum::Router {
    build_app_for_tests().await.expect("build test app")
}

#[tokio::test]
async fn playground_basic_allow_and_deny() {
    let app = build_test_app().await;

    let body = json!({
        "policies": [
            "permit(principal, action, resource);",
            "forbid(principal == User::\"bob\", action, resource);"
        ],
        "authorization_requests": [
            {"name":"alice-allow","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\""},
            {"name":"bob-deny","principal":"User::\"bob\"","action":"Action::\"view\"","resource":"Resource::\"doc1\""}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Optionally parse and assert decisions later
}

#[tokio::test]
async fn playground_entities_and_parents_forbid() {
    let app = build_test_app().await;

    let body = json!({
        "policies": [
            "forbid(principal in Group::\"admins\", action, resource);",
            "permit(principal, action, resource);"
        ],
        "entities": [
            {"uid":"User::\"alice\"","attributes":{},"parents":["Group::\"admins\""]},
            {"uid":"Group::\"admins\"","attributes":{},"parents":[]}
        ],
        "authorization_requests": [
            {"name":"alice-deny","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\""}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn playground_context_affects_decision() {
    let app = build_test_app().await;

    let body = json!({
        "policies": [
            "permit(principal, action, resource) when { context.mfa == true };"
        ],
        "authorization_requests": [
            {"name":"no-mfa","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": false}},
            {"name":"with-mfa","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}}
        ]
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
