use axum::{body::Body, http::{Request, StatusCode}};
use serde_json::json;
use tower::ServiceExt;

use hodei_artifacts_api::build_app_for_tests;

async fn build_test_app() -> axum::Router {
    build_app_for_tests().await.expect("build test app")
}

#[tokio::test]
async fn playground_multiple_scenarios_ok() {
    let app = build_test_app().await;

    let body = json!({
        "policies": [
            "permit(principal, action, resource) when { context.mfa == true };"
        ],
        "schema": null,
        "entities": [],
        "authorization_requests": [
            {"name":"s1","principal":"User::\"alice\"","action":"Action::\"view\"","resource":"Resource::\"doc1\"","context": {"mfa": true}},
            {"name":"s2","principal":"User::\"bob\"","action":"Action::\"view\"","resource":"Resource::\"doc2\"","context": {"mfa": true}}
        ],
        "options": {"include_diagnostics": true, "include_policy_traces": false}
    });

    let req = Request::builder()
        .uri("/api/v1/policies/playground")
        .method("POST")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();

    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
