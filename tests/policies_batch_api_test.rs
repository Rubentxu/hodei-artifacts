use axum::{body::Body, http::{Request, StatusCode}};
use serde_json::json;
use tower::ServiceExt;

use hodei_artifacts_api::build_app_for_tests;

async fn build_test_app() -> axum::Router {
    build_app_for_tests().await.expect("build test app")
}

#[tokio::test]
async fn batch_basic_two_scenarios() {
    let app = build_test_app().await;

    let body = json!({
        "policies": [
            "permit(principal, action, resource) when { context.mfa == true };",
            "forbid(principal == User::\"bob\", action, resource);"
        ],
        "entities": [],
        "schema": null,
        "scenarios": [
            {"name":"alice-allow","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}},
            {"name":"bob-deny","principal":"User::\"bob\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}}
        ],
        "limit_scenarios": null,
        "timeout_ms": null
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground/batch")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
