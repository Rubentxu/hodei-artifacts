use axum::{body::{Body, to_bytes}, http::{Request, StatusCode}};
use serde_json::json;
use tower::ServiceExt;

use hodei_artifacts_api::build_app_for_tests;

async fn build_test_app() -> axum::Router {
    build_app_for_tests().await.expect("build test app")
}

#[tokio::test]
async fn batch_respects_limit_scenarios() {
    let app = build_test_app().await;

    let mut scenarios = Vec::new();
    for i in 0..10 {
        scenarios.push(json!({
            "name": format!("s-{}", i),
            "principal": "User::\"u\"",
            "action": "Action::\"view\"",
            "resource": "Resource::\"r\"",
            "context": {"mfa": true}
        }));
    }

    let body = json!({
        "policies": [
            "permit(principal, action, resource) when { context.mfa == true };",
        ],
        "entities": [],
        "schema": null,
        "scenarios": scenarios,
        "limit_scenarios": 3,
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
    // Optionally, parse body and assert results_count == 3
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["results_count"].as_u64().unwrap(), 3);
}
